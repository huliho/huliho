// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { act, fireEvent, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, expect, test, vi } from "vitest";

import { commandsSnapshot } from "../../commands/registry";
import { ToastProvider, Toasts } from "../../design-system/toast";
import { stubWidthQueries } from "../../shell/width-queries-rig";
import { fixtureCache } from "../fixture-cache";
import { INBOX_ID, INBOX_PAGE } from "../fixtures";
import { command, focused, inbox, mockListBox, renderList, row } from "../list-rig";
import type { Options } from "../list-rig";
import { prefetchMenus } from "../menus-chunk";
import type { OpenFocus } from "../thread-history";
import { LONG_PRESS_MS } from "./long-press";
import { closeSelection, startSelection } from "./selection-lend";
import { WRITABLE, boxOf, rowAt } from "./writable-rig";

function WithToasts({ children }: { children: ReactNode }) {
  return (
    <ToastProvider>
      {children}
      <Toasts />
    </ToastProvider>
  );
}

// The list of `role` over the fixture page with the bar's chunk in, the
// focus on its first row.
async function listed(role: string, options: Options = {}) {
  const cache = fixtureCache({ [`${boxOf(role).id}/0`]: INBOX_PAGE });
  await act(async () => {
    await prefetchMenus();
  });
  renderList({ mailbox: boxOf(role), mailboxes: WRITABLE, cache, wrapper: WithToasts, ...options });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((entry) => entry.id === "selection.toggle")).toBe(true);
  });
  act(() => {
    row(0).focus();
  });
  return cache;
}

// Which rows say they are picked, by place.
function picked(): number[] {
  return [0, 1, 2, 3, 4, 5].filter((index) => row(index).getAttribute("aria-selected") === "true");
}

function checkOf(index: number): HTMLElement {
  return within(row(index)).getByRole("checkbox", { name: "Select conversation" });
}

function bar(): HTMLElement {
  return screen.getByRole("toolbar");
}

// The names of the bar's buttons, in order.
function barButtons(): (string | null)[] {
  return within(bar())
    .getAllByRole("button")
    .map((button) => button.getAttribute("aria-label"));
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
});

test("x picks the cursor's row: every row shows its check, the bar names the count and Escape closes the mode", async () => {
  await listed("inbox");
  expect(within(row(1)).queryByRole("checkbox")).toBeNull();
  command("x");
  await vi.waitFor(() => {
    expect(picked()).toEqual([0]);
  });
  const grid = screen.getByRole("grid");
  expect(grid.getAttribute("aria-multiselectable")).toBe("true");
  expect(row(1).getAttribute("aria-selected")).toBe("false");
  expect(checkOf(1).getAttribute("aria-checked")).toBe("false");
  expect(checkOf(0).getAttribute("aria-checked")).toBe("true");
  expect((await screen.findByRole("toolbar")).getAttribute("aria-label")).toBe("1 selected");
  // The status region says the count as the bar appears.
  await vi.waitFor(() => {
    expect(screen.getByText("1 selected")).toBeDefined();
  });
  command("Escape");
  await vi.waitFor(() => {
    expect(grid.hasAttribute("aria-multiselectable")).toBe(false);
  });
  expect(row(0).hasAttribute("aria-selected")).toBe(false);
  expect(screen.queryByRole("toolbar")).toBeNull();
});

test("the check under the pointer picks its row and opens nothing; Shift with a click picks the run", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  await listed("inbox", { onOpen: opened });
  act(() => {
    fireEvent.pointerOver(row(1), { pointerType: "mouse" });
  });
  const box = checkOf(1);
  // The press leaves the focus where it stood.
  expect(fireEvent.mouseDown(box)).toBe(false);
  fireEvent.click(box);
  await vi.waitFor(() => {
    expect(picked()).toEqual([1]);
  });
  fireEvent.click(row(4), { shiftKey: true });
  await vi.waitFor(() => {
    expect(picked()).toEqual([1, 2, 3, 4]);
  });
  expect(opened).not.toHaveBeenCalled();
  expect(focused()).toBe(row(0));
});

test("Shift with a click and no row toggled yet picks the run from the cursor's row", async () => {
  await listed("inbox");
  fireEvent.click(row(2), { shiftKey: true });
  await vi.waitFor(() => {
    expect(picked()).toEqual([0, 1, 2]);
  });
});

test("the keys act on the picks: one move with one toast for every thread, then the mode closes", async () => {
  const cache = await listed("inbox");
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  await vi.waitFor(() => {
    expect(picked()).toEqual([0, 2]);
  });
  command("e");
  await vi.waitFor(() => {
    expect(screen.getAllByText("2 conversations archived.")).toHaveLength(2);
  });
  expect(cache.batches).toHaveLength(1);
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual([
    ...rowAt(0).memberIds,
    ...rowAt(2).memberIds,
  ]);
  expect(screen.getByRole("grid").hasAttribute("aria-multiselectable")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(cache.batches[1]?.map((mutation) => mutation.id)).toEqual([
    ...rowAt(0).memberIds,
    ...rowAt(2).memberIds,
  ]);
});

test("the read flip reads mark as read while a pick is unread and marks only those; star takes the unstarred", async () => {
  const cache = await listed("inbox");
  // The first row is unread, the third read and starred.
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  await screen.findByRole("toolbar");
  expect(barButtons()).toEqual([
    "Archive",
    "Delete",
    "Mark as spam",
    "Mark as read",
    "Move to…",
    "Close selection",
  ]);
  command("u");
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(0).memberIds);
  act(() => {
    row(0).focus();
  });
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  command("s");
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(2);
  });
  expect(cache.batches[1]?.map((mutation) => mutation.id)).toEqual([rowAt(0).memberIds.at(-1)]);
});

test("a press in the bar acts on the picks and hands the focus to the row that takes the cursor's place", async () => {
  const cache = await listed("inbox");
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  const archive = within(await screen.findByRole("toolbar")).getByRole("button", {
    name: "Archive",
  });
  act(() => {
    archive.focus();
  });
  fireEvent.click(archive);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  await vi.waitFor(() => {
    expect(screen.queryByRole("toolbar")).toBeNull();
  });
  // A row above the cursor's left with it, so the cursor goes to the row
  // after its own; the fixture's rows stay where they stood.
  await vi.waitFor(() => {
    expect(focused()).toBe(row(3));
  });
});

test("in Junk the bar holds delete forever and not spam, and no archive", async () => {
  await listed("junk");
  command("x");
  await screen.findByRole("toolbar");
  expect(barButtons()).toEqual([
    "Delete forever",
    "Not spam",
    "Mark as read",
    "Move to…",
    "Close selection",
  ]);
});

test("under touch the bar keeps archive, delete and the read flip, with spam behind More actions", async () => {
  stubWidthQueries({ wide: true, coarse: true });
  await listed("inbox");
  command("x");
  await screen.findByRole("toolbar");
  expect(barButtons()).toEqual([
    "Archive",
    "Delete",
    "Mark as read",
    "More actions",
    "Close selection",
  ]);
});

test("an action behind More actions acts on the picks once the menu has closed and hands the focus to a row", async () => {
  stubWidthQueries({ wide: true, coarse: true });
  const cache = await listed("inbox");
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  const more = within(await screen.findByRole("toolbar")).getByRole("button", {
    name: "More actions",
  });
  fireEvent.click(more);
  fireEvent.click(await screen.findByRole("menuitem", { name: "Mark as spam" }));
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual([
    ...rowAt(0).memberIds,
    ...rowAt(2).memberIds,
  ]);
  await vi.waitFor(() => {
    expect(screen.queryByRole("toolbar")).toBeNull();
  });
  await vi.waitFor(() => {
    expect(focused()).toBe(row(3));
  });
});

test("a touch that rests on a row picks it and opens nothing; a tap then picks another", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  await listed("inbox", { onOpen: opened });
  vi.useFakeTimers();
  try {
    fireEvent.pointerDown(row(1), { pointerType: "touch", isPrimary: true });
    expect(row(1).dataset["pressing"]).toBe("true");
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    fireEvent.pointerUp(row(1), { pointerType: "touch" });
    fireEvent.click(row(1));
    expect(picked()).toEqual([1]);
    fireEvent.pointerDown(row(3), { pointerType: "touch", isPrimary: true });
    fireEvent.pointerUp(row(3), { pointerType: "touch" });
    fireEvent.click(row(3));
    expect(picked()).toEqual([1, 3]);
    expect(opened).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

test("a second finger ends a pending press, so nothing is picked", async () => {
  await listed("inbox");
  vi.useFakeTimers();
  try {
    fireEvent.pointerDown(row(1), { pointerType: "touch", isPrimary: true });
    expect(row(1).dataset["pressing"]).toBe("true");
    fireEvent.pointerDown(row(2), { pointerType: "touch", isPrimary: false });
    expect(row(1).dataset["pressing"]).toBeUndefined();
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
  } finally {
    vi.useRealTimers();
  }
  expect(picked()).toEqual([]);
  expect(screen.queryByRole("toolbar")).toBeNull();
});

test("a touch lifted before its press lands opens the row", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  await listed("inbox", { onOpen: opened });
  vi.useFakeTimers();
  try {
    fireEvent.pointerDown(row(1), { pointerType: "touch", isPrimary: true });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS - 1);
    });
    fireEvent.pointerUp(row(1), { pointerType: "touch" });
    fireEvent.click(row(1));
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
  } finally {
    vi.useRealTimers();
  }
  expect(picked()).toEqual([]);
  expect(opened).toHaveBeenCalledExactlyOnceWith(rowAt(1), "title");
});

test("Select opens the mode with nothing picked; the thread's Escape closes a standing mode first", async () => {
  await listed("inbox");
  act(() => {
    startSelection();
  });
  await vi.waitFor(() => {
    expect(screen.getByRole("grid").getAttribute("aria-multiselectable")).toBe("true");
  });
  expect(picked()).toEqual([]);
  expect((await screen.findByRole("toolbar")).getAttribute("aria-label")).toBe("0 selected");
  expect(barButtons()).toEqual(["Close selection"]);
  let closed = false;
  act(() => {
    closed = closeSelection();
  });
  expect(closed).toBe(true);
  await vi.waitFor(() => {
    expect(screen.queryByRole("toolbar")).toBeNull();
  });
  expect(closeSelection()).toBe(false);
});

test("a mailbox that offers no action registers no x and draws no check", async () => {
  const shared = inbox();
  renderList({
    mailbox: {
      ...shared,
      myRights: { ...shared.myRights, maySetSeen: false, maySetKeywords: false },
    },
    cache: fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE }),
  });
  await screen.findByRole("grid");
  act(() => {
    fireEvent.pointerOver(row(1), { pointerType: "mouse" });
  });
  expect(within(row(1)).queryByRole("checkbox")).toBeNull();
  expect(commandsSnapshot().some((entry) => entry.id === "selection.toggle")).toBe(false);
  act(() => {
    startSelection();
  });
  expect(screen.getByRole("grid").hasAttribute("aria-multiselectable")).toBe(false);
});

test("an open thread among the picks gives way to the next thread that stays", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  await listed("inbox", { openThreadId: rowAt(1).threadId, onOpen: opened });
  act(() => {
    row(1).focus();
  });
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  await vi.waitFor(() => {
    expect(picked()).toEqual([1, 2]);
  });
  command("#");
  await vi.waitFor(() => {
    expect(opened).toHaveBeenCalledExactlyOnceWith(rowAt(3), "list");
  });
});

test("the open thread's row stays the current one while the picks are the selected rows", async () => {
  await listed("inbox", { openThreadId: rowAt(2).threadId });
  expect(row(2).getAttribute("aria-selected")).toBe("true");
  command("x");
  await vi.waitFor(() => {
    expect(picked()).toEqual([0]);
  });
  expect(row(2).getAttribute("aria-selected")).toBe("false");
  expect(row(2).getAttribute("aria-current")).toBe("true");
});
