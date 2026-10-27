// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MemberState, ThreadDetail } from "@huliho/core";
import { act, cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { INBOX_ID, THREAD, threadDetail } from "../fixtures";
import { mockPaneBox, openIn, renderPane } from "../pane-rig";
import { lendOpenThreadRun } from "./open-thread";
import { WRITABLE } from "./writable-rig";

// Base UI waits this long under a resting pointer before a hint opens.
const HOVER_DELAY_MS = 600;
const INBOX = openIn(INBOX_ID, WRITABLE);
const TRASH = openIn("mb-trash", WRITABLE);
// The toolbar of a thread that acts from a writable inbox, Close last.
const INBOX_BAR = [
  "Archivee",
  "Delete#",
  "Mark as spam!",
  "Move to…m",
  "Mark as unread",
  "Add star",
  "Close conversation",
];
const cleanups: (() => void)[] = [];

function lent() {
  const run = vi.fn<(id: string) => void>();
  cleanups.push(lendOpenThreadRun(run));
  return run;
}

// The toolbar's buttons by name, Close last.
async function toolbar(): Promise<string[]> {
  await screen.findByRole("heading", { level: 2 });
  const close = screen.getByRole("button", { name: "Close conversation" });
  const bar = close.parentElement;
  if (bar === null) {
    throw new Error("Close stands in no toolbar");
  }
  return within(bar)
    .getAllByRole("button")
    .map((button) => button.textContent.trim() || (button.getAttribute("aria-label") ?? ""));
}

// The fixture thread moved whole into `mailboxId`, its newest message starred.
function starredIn(detail: ThreadDetail, mailboxId: string): ThreadDetail {
  const newest = detail.thread.emailIds.at(-1);
  const members = Object.entries(detail.thread.members).map(
    ([id, member]): [string, MemberState] => [
      id,
      {
        mailboxIds: { [mailboxId]: true },
        keywords: id === newest ? { ...member.keywords, $flagged: true } : member.keywords,
      },
    ],
  );
  return { ...detail, thread: { ...detail.thread, members: Object.fromEntries(members) } };
}

mockPaneBox();

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  for (const off of cleanups.splice(0)) {
    off();
  }
});

test("the toolbar heads the pane with what the mailbox the thread acts from offers, and a press acts on the open thread", async () => {
  const run = lent();
  renderPane(THREAD, { open: INBOX });
  expect(await toolbar()).toEqual(INBOX_BAR);
  fireEvent.click(screen.getByRole("button", { name: "Archive" }));
  expect(run).toHaveBeenCalledExactlyOnceWith("thread.archive");
});

test("an action the mailbox does not offer is absent, never grayed", async () => {
  // The fixtures' inbox lets a reader mark mail and move none of it.
  renderPane(THREAD, { open: openIn(INBOX_ID) });
  expect(await toolbar()).toEqual(["Mark as unread", "Add star", "Close conversation"]);
});

test("a thread with no message left in the mailbox it acts from offers no action", async () => {
  renderPane(THREAD, { open: TRASH });
  expect(await toolbar()).toEqual(["Close conversation"]);
});

test("the words follow the mailbox and the thread: delete forever in Trash, mark as read and remove star", async () => {
  renderPane(starredIn(threadDetail([4]), "mb-trash"), { open: TRASH });
  expect(await toolbar()).toEqual([
    "Delete forever#",
    "Mark as spam!",
    "Move to…m",
    "Mark as read",
    "Remove star",
    "Close conversation",
  ]);
});

test("as a screen the actions follow the way back and its key, parted by a rule", async () => {
  renderPane(THREAD, { position: "screen", open: INBOX });
  await screen.findByRole("heading", { level: 1 });
  const back = screen.getByRole("button", { name: "Back to Inbox" });
  const key = back.nextElementSibling;
  expect(key?.textContent).toBe("esc");
  const actions = key?.nextElementSibling;
  if (!(actions instanceof HTMLElement)) {
    throw new Error("no actions after the way back");
  }
  expect(actions.hasAttribute("data-parted")).toBe(true);
  expect(within(actions).getAllByRole("button")).toHaveLength(INBOX_BAR.length - 1);
});

test("a resting pointer opens a button's tooltip with its label and its key", async () => {
  renderPane(THREAD, { open: INBOX });
  await toolbar();
  vi.useFakeTimers();
  const star = screen.getByRole("button", { name: "Add star" });
  fireEvent.pointerEnter(star, { pointerType: "mouse" });
  fireEvent.mouseEnter(star);
  fireEvent.mouseMove(star);
  act(() => {
    vi.advanceTimersByTime(HOVER_DELAY_MS);
  });
  expect(screen.getAllByText("Add star").map((element) => element.textContent)).toContain(
    "Add stars",
  );
});
