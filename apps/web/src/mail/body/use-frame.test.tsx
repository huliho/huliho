// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { ESCAPE } from "../../commands/keys";
import { registerCommand } from "../../commands/registry";
import { FRAME_SANDBOX } from "./frame-document";
import { FRAME_HEIGHT_MAX, useFrame } from "./use-frame";

// The observers the hook made, each with what it watches.
interface Observer {
  measure: () => void;
  watched: Element[];
  live: boolean;
}

const observers: Observer[] = [];

class FakeResizeObserver {
  private readonly entry: Observer;

  constructor(measure: () => void) {
    this.entry = { measure, watched: [], live: true };
    observers.push(this.entry);
  }

  observe(element: Element): void {
    this.entry.watched.push(element);
  }

  disconnect(): void {
    this.entry.live = false;
  }
}

function Harness({ html }: { html: string }) {
  const { ref, height } = useFrame(html);
  // The harness names the strictest sandbox; the hook sets the frame's own.
  return <iframe ref={ref} title="mail" sandbox="" data-height={String(height)} />;
}

function frame(): HTMLIFrameElement {
  const found = screen.getByTitle("mail");
  if (!(found instanceof HTMLIFrameElement)) {
    throw new TypeError("the harness renders no frame");
  }
  return found;
}

// The frame once its document loaded. The test environment loads no
// srcdoc, so the event is sent by hand and the hook follows the empty
// document the frame holds.
function loaded(): HTMLIFrameElement {
  fireEvent.load(frame());
  return frame();
}

interface Sizes {
  // The height of the document's content.
  content: number;
  // The height the frame has; the content's own unless said.
  frame?: number;
  // The height of a scrollbar along the frame's bottom edge.
  scrollbar?: number;
}

// Gives the frame's document the sizes a layout would; the test
// environment lays nothing out.
function layout({ content, frame: height = content, scrollbar = 0 }: Sizes): void {
  const view = frame().contentWindow;
  const root = frame().contentDocument?.documentElement;
  if (view === null || root === undefined) {
    throw new TypeError("the frame holds no document");
  }
  Object.defineProperty(root, "offsetHeight", { configurable: true, value: content });
  Object.defineProperty(root, "clientHeight", { configurable: true, value: height - scrollbar });
  Object.defineProperty(view, "innerHeight", { configurable: true, value: height });
}

// The sizes changed and the observer hears of it.
function resize(sizes: Sizes): void {
  layout(sizes);
  act(() => {
    observers.findLast((observer) => observer.live)?.measure();
  });
}

function live(): Observer[] {
  return observers.filter((observer) => observer.live);
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  observers.length = 0;
});

test("the frame takes the document as srcdoc with no source and has no height before it loaded", () => {
  render(<Harness html="<p>hi</p>" />);
  expect(frame().srcdoc).toBe("<p>hi</p>");
  expect(frame().hasAttribute("src")).toBe(false);
  expect(frame().dataset["height"]).toBe("null");
});

test("the hook sandboxes the frame itself, before the document goes in", () => {
  const sandboxed: (string | null)[] = [];
  const assigned = vi.spyOn(HTMLIFrameElement.prototype, "srcdoc", "set");
  assigned.mockImplementation(function record(this: HTMLIFrameElement) {
    sandboxed.push(this.getAttribute("sandbox"));
  });
  render(<Harness html="<p>hi</p>" />);
  assigned.mockRestore();
  expect(sandboxed).toEqual(["allow-same-origin allow-popups allow-popups-to-escape-sandbox"]);
  expect(frame().getAttribute("sandbox")).toBe(FRAME_SANDBOX);
});

test("a loaded document is measured at once and watched by its body", () => {
  render(<Harness html="<p>hi</p>" />);
  layout({ content: 120, frame: 0 });
  const target = loaded();
  expect(target.dataset["height"]).toBe("120");
  expect(live()).toHaveLength(1);
  expect(live()[0]?.watched).toEqual([target.contentDocument?.body]);
});

test("the height follows the content up and down, whatever height the frame has", () => {
  render(<Harness html="<p>hi</p>" />);
  const target = loaded();
  resize({ content: 640, frame: 0 });
  expect(target.dataset["height"]).toBe("640");
  resize({ content: 300, frame: 640 });
  expect(target.dataset["height"]).toBe("300");
  resize({ content: 300, frame: 300 });
  expect(target.dataset["height"]).toBe("300");
});

test("a mail wider than the frame gets room for the scrollbar along its bottom edge", () => {
  render(<Harness html="<p>hi</p>" />);
  const target = loaded();
  resize({ content: 400, frame: 400, scrollbar: 15 });
  expect(target.dataset["height"]).toBe("415");
  resize({ content: 400, frame: 415, scrollbar: 15 });
  expect(target.dataset["height"]).toBe("415");
});

test("past its bound the frame stops growing and its document scrolls inside", () => {
  render(<Harness html="<p>hi</p>" />);
  const target = loaded();
  const body = target.contentDocument?.body.style;
  expect(body?.getPropertyValue("overflow-y")).toBe("");
  resize({ content: FRAME_HEIGHT_MAX + 5000 });
  expect(target.dataset["height"]).toBe(String(FRAME_HEIGHT_MAX));
  expect(body?.getPropertyValue("overflow-y")).toBe("auto");
  expect(body?.getPropertyPriority("overflow-y")).toBe("important");
  resize({ content: FRAME_HEIGHT_MAX });
  expect(target.dataset["height"]).toBe(String(FRAME_HEIGHT_MAX));
  expect(body?.getPropertyValue("overflow-y")).toBe("");
});

test("the app's keys work with the focus inside the frame, so Escape closes the thread", () => {
  const close = vi.fn<() => void>();
  const unregister = registerCommand({
    id: "thread.close",
    label: "Close",
    group: "navigate",
    keys: [ESCAPE],
    run: close,
  });
  render(<Harness html="<p>hi</p>" />);
  const view = loaded().contentWindow;
  view?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
  expect(close).toHaveBeenCalledOnce();
  cleanup();
  view?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
  expect(close).toHaveBeenCalledOnce();
  unregister();
});

test("a document that loads again is followed once", () => {
  render(<Harness html="<p>hi</p>" />);
  loaded();
  loaded();
  expect(live()).toHaveLength(1);
});

test("a new document replaces the one shown and the old one is let go", () => {
  const { rerender } = render(<Harness html="<p>first</p>" />);
  loaded();
  rerender(<Harness html="<p>second</p>" />);
  expect(frame().srcdoc).toBe("<p>second</p>");
  expect(live()).toHaveLength(0);
  loaded();
  expect(live()).toHaveLength(1);
});

test("a frame that leaves the page stops following its document", () => {
  render(<Harness html="<p>hi</p>" />);
  loaded();
  cleanup();
  expect(live()).toHaveLength(0);
});
