// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { commandsSnapshot, registerCommand } from "../commands/registry";
import { htmlDetail, textDetail } from "./body-fixtures";
import { stubServer } from "./card-rig";
import { fixtureCache } from "./fixture-cache";
import { THREAD, THREAD_ID, threadDetail } from "./fixtures";
import { cards, headOf, mockPaneBox, renderPane } from "./pane-rig";
import { PLAIN_LINES_MAX } from "./plain-text";

const DETAILS = "Message details";
const PLAIN_TAB = "Plain text";
const UNREAD_AT = 4;
const NEWEST_ID = "e-3";

mockPaneBox();

const cleanups: (() => void)[] = [];

beforeEach(() => {
  stubServer();
});

afterEach(() => {
  cleanup();
  for (const off of cleanups.splice(0)) {
    off();
  }
});

function detailsOf(card: HTMLElement): HTMLElement {
  return within(card).getByRole("button", { name: DETAILS });
}

function inspectCommand() {
  return commandsSnapshot().findLast((one) => one.id === "message.inspect");
}

// The text of the card's message, which its details show on the first tab.
function textOf(card: HTMLElement): string {
  return within(card).getByText(/\S/, { selector: "[dir='auto'] div" }).textContent;
}

// The frame a message renders in, inside the scope given.
async function frameIn(scope: HTMLElement): Promise<HTMLIFrameElement> {
  const found = await within(scope).findByTitle(/^Message from/);
  if (!(found instanceof HTMLIFrameElement)) {
    throw new TypeError("no frame renders the message");
  }
  return found;
}

async function closed(): Promise<void> {
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

// A key pressed inside the frame, which reaches the frame's window with
// that window as its view.
function pressIn(frame: HTMLIFrameElement, key: string): void {
  const view = frame.contentWindow;
  if (view === null) {
    throw new Error("the frame has no window");
  }
  view.dispatchEvent(new KeyboardEvent("keydown", { key, view }));
}

// The newest card of the pane once its heading shows.
async function newestCard(): Promise<HTMLElement> {
  await screen.findByRole("heading", { level: 2 });
  const newest = cards().at(-1);
  if (newest === undefined) {
    throw new Error("the pane has no card");
  }
  return newest;
}

// The plain tab of the card's details, opened from its head.
async function plainTabOf(card: HTMLElement): Promise<HTMLElement> {
  fireEvent.click(detailsOf(card));
  const dialog = await screen.findByRole("dialog", { name: DETAILS });
  fireEvent.click(within(dialog).getByRole("tab", { name: PLAIN_TAB }));
  return within(dialog).getByRole("tabpanel");
}

test("the details button of an open card opens its inspector; Escape closes it and the button takes the focus back, also where a click never gave it the focus", async () => {
  renderPane();
  const newest = await newestCard();
  const text = await within(newest).findByText(/De meerprijs/);
  const button = detailsOf(newest);
  // An engine that leaves a clicked button without the focus.
  expect(document.activeElement).not.toBe(button);
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", { name: DETAILS });
  expect(within(dialog).getByRole("tabpanel").textContent).toBe(text.textContent);
  // A whole text stands on the plain tab with no sentence under it.
  fireEvent.click(within(dialog).getByRole("tab", { name: PLAIN_TAB }));
  expect(within(dialog).getByRole("tabpanel").textContent).toBe(text.textContent);
  await closed();
  await waitFor(() => {
    expect(document.activeElement).toBe(button);
  });
  // A folded card carries no details button.
  expect(within(cards()[0] ?? newest).queryByRole("button", { name: DETAILS })).toBeNull();
});

test("the palette command inspects the open card the focus is in or else the newest open card; it stands only while a card is open", async () => {
  renderPane(threadDetail([UNREAD_AT]));
  await screen.findByRole("heading", { level: 2 });
  const [folded] = cards();
  const unread = cards()[UNREAD_AT - 2];
  const newest = cards().at(-1);
  if (folded === undefined || unread === undefined || newest === undefined) {
    throw new Error("the pane lacks its cards");
  }
  await within(newest).findByText(/De meerprijs/);
  await within(unread).findByText(/\S/, { selector: "[dir='auto'] div" });
  expect(inspectCommand()?.keys).toEqual([]);
  // With the focus outside every open card the newest open card is the one.
  headOf(folded).focus();
  inspectCommand()?.run();
  let dialog = await screen.findByRole("dialog", { name: DETAILS });
  expect(within(dialog).getByRole("tabpanel").textContent).toBe(textOf(newest));
  await closed();
  headOf(unread).focus();
  inspectCommand()?.run();
  dialog = await screen.findByRole("dialog", { name: DETAILS });
  expect(within(dialog).getByRole("tabpanel").textContent).toBe(textOf(unread));
  await closed();
  // With every card folded the command leaves the palette.
  fireEvent.click(headOf(unread));
  fireEvent.click(headOf(newest));
  expect(inspectCommand()).toBeUndefined();
  fireEvent.click(headOf(newest));
  expect(inspectCommand()).toBeDefined();
});

test("inside the inspector's frame a key runs nothing and Escape closes the inspector alone, leaving the thread open", async () => {
  const cache = fixtureCache(
    {},
    { [THREAD_ID]: THREAD },
    { [NEWEST_ID]: htmlDetail(NEWEST_ID, "<p>marked up</p>") },
  );
  const { onClose } = renderPane(THREAD, { cache });
  const next = vi.fn<() => void>();
  cleanups.push(
    registerCommand({
      id: "list.next",
      label: "Next",
      group: "navigate",
      keys: [{ key: "j" }],
      run: next,
    }),
  );
  const newest = await newestCard();
  const own = await frameIn(newest);
  fireEvent.load(own);
  fireEvent.click(detailsOf(newest));
  const dialog = await screen.findByRole("dialog", { name: DETAILS });
  const frame = await frameIn(dialog);
  fireEvent.load(frame);
  pressIn(frame, "j");
  expect(next).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog", { name: DETAILS })).toBe(dialog);
  pressIn(frame, "Escape");
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  expect(onClose).not.toHaveBeenCalled();
  // Outside the inspector the same keys reach the list and close the thread.
  pressIn(own, "j");
  expect(next).toHaveBeenCalledOnce();
  pressIn(own, "Escape");
  expect(onClose).toHaveBeenCalledOnce();
});

test("the plain tab stops where the card stops, with the sentence and no button: at the server's cut and past the lines the card draws", async () => {
  const cut = fixtureCache(
    {},
    { [THREAD_ID]: THREAD },
    { [NEWEST_ID]: textDetail(NEWEST_ID, "the first part", { cut: true }) },
  );
  renderPane(THREAD, { cache: cut });
  const panel = await plainTabOf(await newestCard());
  expect(panel.textContent).toBe("the first partThis message was cut short at 4 MB.");
  expect(within(panel).queryByRole("button", { name: "Show the whole message" })).toBeNull();
  expect(within(panel).queryByRole("link", { name: "Download the message" })).toBeNull();
  cleanup();
  const long = fixtureCache(
    {},
    { [THREAD_ID]: THREAD },
    { [NEWEST_ID]: textDetail(NEWEST_ID, `${"line\n".repeat(PLAIN_LINES_MAX)}past the bound`) },
  );
  renderPane(THREAD, { cache: long });
  const bounded = await plainTabOf(await newestCard());
  expect(bounded.textContent).toContain("This message is too long to show in full.");
  expect(bounded.textContent).not.toContain("past the bound");
});
