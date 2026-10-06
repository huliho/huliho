// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";

import { installCommandListener } from "../../commands/registry";
import { FRAME_SANDBOX } from "./frame-document";

// The tallest a frame grows with its mail, in CSS pixels; a mail taller
// than that scrolls inside the frame.
export const FRAME_HEIGHT_MAX = 20_000;

// The height the document needs: the root's own box, which follows the
// content down as well as up whatever height the frame has, plus the
// scrollbar a mail wider than the frame gets along its bottom edge.
function contentHeight(page: Document, view: Window): number {
  const root = page.documentElement;
  const scrollbar = Math.max(view.innerHeight - root.clientHeight, 0);
  return root.offsetHeight + scrollbar;
}

// The frame's height for its document. The base style keeps a document
// from scrolling down, which holds the measure still; one taller than
// the bound has to scroll to be read, whatever its own styles say.
function fit(page: Document, view: Window): number {
  const needed = contentHeight(page, view);
  if (needed > FRAME_HEIGHT_MAX) {
    page.body.style.setProperty("overflow-y", "auto", "important");
  } else {
    page.body.style.removeProperty("overflow-y");
  }
  return Math.min(needed, FRAME_HEIGHT_MAX);
}

// Follows one loaded document: its height now and whenever its body
// changes size, plus the app's keys while the focus is inside it, so
// Escape closes the thread from there. An engine that hands the host no
// event from a sandboxed document leaves the keys to the app around it.
function follow(frame: HTMLIFrameElement, report: (height: number) => void): () => void {
  const page = frame.contentDocument;
  const view = frame.contentWindow;
  if (page === null || view === null) {
    return () => undefined;
  }
  const measure = (): void => {
    report(fit(page, view));
  };
  measure();
  const observer = new ResizeObserver(measure);
  observer.observe(page.body);
  const uninstall = installCommandListener(view);
  return () => {
    observer.disconnect();
    uninstall();
  };
}

// Calls `parsed` once the frame holds a document other than `previous`
// that is past parsing, looking once per drawn frame; the returned
// function stops the looking. A document is past parsing well ahead of
// its load event, which waits for every image.
function onceParsed(
  frame: HTMLIFrameElement,
  previous: Document | null,
  parsed: () => void,
): () => void {
  let handle = 0;
  const look = (): void => {
    const page = frame.contentDocument;
    if (page !== null && page !== previous && page.readyState !== "loading") {
      parsed();
    } else {
      handle = requestAnimationFrame(look);
    }
  };
  handle = requestAnimationFrame(look);
  return () => {
    cancelAnimationFrame(handle);
  };
}

export interface Frame {
  // For the iframe element, which takes no source; whatever sandbox it
  // names, the hook sets the frame's own.
  ref: RefObject<HTMLIFrameElement | null>;
  // The height the frame should have; null until its document is parsed.
  height: number | null;
}

// Renders a built document in the frame the ref names, sandboxed before
// `srcdoc` goes in. It follows the document once parsed: as tall as its
// content, the app's keys inside; the load event stands in where no frame is drawn.
export function useFrame(html: string): Frame {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState<number | null>(null);
  useEffect(() => {
    const frame = ref.current;
    if (frame === null) {
      return undefined;
    }
    let unfollow = (): void => undefined;
    const shown = (): void => {
      unfollow();
      unfollow = follow(frame, setHeight);
    };
    const previous = frame.contentDocument;
    frame.setAttribute("sandbox", FRAME_SANDBOX);
    frame.srcdoc = html;
    const stopLooking = onceParsed(frame, previous, shown);
    const loaded = (): void => {
      stopLooking();
      shown();
    };
    frame.addEventListener("load", loaded);
    return () => {
      stopLooking();
      frame.removeEventListener("load", loaded);
      unfollow();
    };
  }, [html]);
  return { ref, height };
}
