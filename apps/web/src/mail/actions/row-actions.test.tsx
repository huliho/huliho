// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

import { commandsSnapshot } from "../../commands/registry";
import { fixtureCache } from "../fixture-cache";
import { INBOX_ID, INBOX_PAGE } from "../fixtures";
import { focused, inbox, mockListBox, renderList, row } from "../list-rig";
import type { OpenFocus } from "../thread-history";
import { ROW_ACTIONS } from "./row-actions";
import { WRITABLE, boxOf, rowAt } from "./writable-rig";

// The list of `role` over the fixture page, once it registered its Act group.
async function listed(role: string, options: Parameters<typeof renderList>[0] = {}) {
  const cache = fixtureCache({ [`${boxOf(role).id}/0`]: INBOX_PAGE });
  renderList({ mailbox: boxOf(role), mailboxes: WRITABLE, cache, ...options });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((command) => command.id === "thread.star")).toBe(true);
  });
  return cache;
}

function hover(index: number, pointerType = "mouse"): void {
  act(() => {
    fireEvent.pointerOver(row(index), { pointerType });
  });
}

// Each action of a row by its name, with the key it shows.
function actionsOf(index: number): string[] {
  return within(row(index))
    .queryAllByRole("button")
    .map((button) => `${button.getAttribute("aria-label") ?? ""} ${button.textContent}`);
}

// Presses a row's action; false when the press kept the focus where it stood.
function press(index: number, name: string): boolean {
  const button = within(row(index)).getByRole("button", { name });
  const kept = fireEvent.mouseDown(button);
  fireEvent.click(button);
  return kept;
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
});

test("the row under the pointer carries archive, delete, the read flip and star with their keys", async () => {
  await listed("inbox");
  hover(2);
  const third = rowAt(2);
  expect(actionsOf(2)).toEqual([
    "Archive e",
    "Delete #",
    `${third.unread ? "Mark as read" : "Mark as unread"} u`,
    `${third.flagged ? "Remove star" : "Add star"} s`,
  ]);
  // The buttons stay out of the tab order; the grid keeps its one stop.
  expect(
    within(row(2))
      .getAllByRole("button")
      .map((button) => button.tabIndex),
  ).toEqual([-1, -1, -1, -1]);
});

test("the strip follows the pointer but never a touch; the cursor's row keeps one for its focus", async () => {
  await listed("inbox");
  hover(2, "touch");
  expect(actionsOf(2)).toEqual([]);
  hover(2);
  hover(3);
  expect(actionsOf(2)).toEqual([]);
  expect(actionsOf(3)).toHaveLength(4);
  act(() => {
    fireEvent.pointerLeave(screen.getByRole("grid"));
  });
  expect(actionsOf(3)).toEqual([]);
  expect(actionsOf(0)).toHaveLength(4);
});

test("a press on a row's archive takes that row's thread, opens nothing and leaves the focus where it stood", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  const cache = await listed("inbox", { onOpen: opened });
  act(() => {
    row(0).focus();
  });
  hover(2);
  expect(press(2, "Archive")).toBe(false);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(2).memberIds);
  expect(cache.batches[0]?.[0]).toMatchObject({
    patch: { [`mailboxIds/${INBOX_ID}`]: null, "mailboxIds/mb-archive": true },
  });
  expect(opened).not.toHaveBeenCalled();
  expect(focused()).toBe(row(0));
});

test("in Trash a row offers delete forever with the flips and no archive", async () => {
  await listed("trash");
  hover(1);
  expect(actionsOf(1).slice(0, 1)).toEqual(["Delete forever #"]);
  expect(actionsOf(1)).toHaveLength(3);
});

test("a press on the open thread's row with the focus away from the list opens the next thread on its title", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  await listed("inbox", { openThreadId: rowAt(0).threadId, onOpen: opened });
  hover(0);
  press(0, "Archive");
  await vi.waitFor(() => {
    expect(opened).toHaveBeenCalledExactlyOnceWith(rowAt(1), "title");
  });
});

test("a mailbox that gives up no email offers the two flips alone on a row", async () => {
  renderList({ mailbox: inbox(), cache: fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE }) });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((command) => command.id === "thread.star")).toBe(true);
  });
  hover(1);
  expect(actionsOf(1).map((action) => action.split(" ").at(-1))).toEqual(["u", "s"]);
  expect(stripOf(1)).not.toBeNull();
});

// The strip a row drew for its buttons; null on a row that drew none.
function stripOf(index: number): Element | null {
  return row(index).querySelector(ROW_ACTIONS);
}

test("a mailbox that offers no action draws no strip on the hovered or the focused row", async () => {
  // A shared mailbox a reader may neither mark nor move in.
  const shared = inbox();
  renderList({
    mailbox: {
      ...shared,
      myRights: { ...shared.myRights, maySetSeen: false, maySetKeywords: false },
    },
  });
  await screen.findByRole("grid");
  expect(commandsSnapshot().some((command) => command.id.startsWith("thread."))).toBe(false);
  act(() => {
    row(0).focus();
  });
  hover(2);
  expect(actionsOf(0)).toEqual([]);
  expect(actionsOf(2)).toEqual([]);
  expect(stripOf(0)).toBeNull();
  expect(stripOf(2)).toBeNull();
});
