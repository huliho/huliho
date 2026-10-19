// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, MailCache } from "@huliho/core";
import { QueryClient } from "@tanstack/react-query";
import { act, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { commandsSnapshot } from "../../commands/registry";
import type { Locale } from "../../paraglide/runtime.js";
import { fixtureCache } from "../fixture-cache";
import { INBOX_ID, INBOX_PAGE } from "../fixtures";
import {
  ROW_PX,
  addMail,
  command,
  coreCache,
  invalidateWindows,
  mockListBox,
  renderList,
  row,
  rows,
  servedInbox,
} from "../list-rig";
import type { OpenFocus } from "../thread-history";
import { LONG_PRESS_MS } from "./long-press";
import { WRITABLE, boxOf, rowAt } from "./writable-rig";

// The rig's rows are this wide; a finger lands this far inside one.
const ROW_WIDTH_PX = 360;
const START_X = 100;
// Past half the row's width, short of half and inside the first quarter.
const PAST_HALF = 200;
const SHORT_OF_HALF = 150;
const A_QUARTER = 60;
// Far enough up or down to leave a row.
const OFF_THE_ROW = ROW_PX;
// Down from a row's top, inside it and far enough from the middle to read as a scroll.
const DOWN_THE_ROW = 40;
// The threads a list over the core's own store holds, a second apart.
const SERVED = 3;

interface Listed {
  role?: string;
  locale?: Locale;
  cache?: MailCache;
  onOpen?: (row: ListRow, focus: OpenFocus) => void;
}

// The list of `role` over the fixture page, once it registered its Act group.
async function listed({ role = "inbox", ...options }: Listed = {}) {
  const cache = fixtureCache({ [`${boxOf(role).id}/0`]: INBOX_PAGE });
  renderList({ mailbox: boxOf(role), mailboxes: WRITABLE, cache, ...options });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((entry) => entry.id === "thread.star")).toBe(true);
  });
  return cache;
}

// A touch at `dx` along the row at `index`, through its middle line.
function at(index: number, dx: number, init: Record<string, unknown> = {}) {
  return {
    pointerType: "touch",
    pointerId: 1,
    isPrimary: true,
    clientX: START_X + dx,
    clientY: index * ROW_PX + ROW_PX / 2,
    ...init,
  };
}

// A finger lands on a row and travels `dx` along it, still down.
function drag(index: number, dx: number, init: Record<string, unknown> = {}): void {
  act(() => {
    fireEvent.pointerDown(row(index), at(index, 0, init));
    fireEvent.pointerMove(row(index), at(index, dx, init));
  });
}

function lift(index: number): void {
  act(() => {
    fireEvent.pointerUp(row(index), at(index, 0));
  });
}

function cellOf(index: number): HTMLElement {
  const cell = row(index).querySelector<HTMLElement>('[role="gridcell"]');
  if (cell === null) {
    throw new Error(`row ${String(index)} holds no cell`);
  }
  return cell;
}

// What the ground behind a row shows: its way and its word; null without one.
function groundOf(index: number): { way: string | undefined; word: string } | null {
  const ground = row(index).querySelector<HTMLElement>("[data-way]");
  return ground === null ? null : { way: ground.dataset["way"], word: ground.textContent };
}

// The list over the core's store and a fake server, once its rows show.
async function served() {
  const client = new QueryClient();
  const server = servedInbox(SERVED, "w", -1);
  const cache = coreCache(server);
  renderList({ mailbox: boxOf("inbox"), mailboxes: WRITABLE, cache, client });
  await vi.waitFor(() => {
    expect(rows()).toHaveLength(SERVED);
  });
  return { client, server, cache };
}

// The pages refetch until `count` rows stand, the swiped thread gone.
async function left(client: QueryClient, count: number): Promise<void> {
  await vi.waitFor(async () => {
    await invalidateWindows(client);
    expect(rows()).toHaveLength(count);
  });
}

function closingMarks(): (string | undefined)[] {
  return rows().map((element) => element.dataset["closing"]);
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
  // jsdom lays nothing out; each row stands at its place in the list.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    return new DOMRect(0, Number(this.dataset["index"] ?? 0) * ROW_PX, ROW_WIDTH_PX, ROW_PX);
  });
});

afterEach(() => {
  vi.useRealTimers();
});

test("a vertical move is the list's scroll: the row never follows, however far the finger goes on", async () => {
  const cache = await listed();
  act(() => {
    fireEvent.pointerDown(row(2), at(2, 0));
    fireEvent.pointerMove(row(2), at(2, 2, { clientY: 2 * ROW_PX + DOWN_THE_ROW }));
    fireEvent.pointerMove(row(2), at(2, PAST_HALF, { clientY: 2 * ROW_PX + DOWN_THE_ROW }));
  });
  lift(2);
  expect(row(2).dataset["swiping"]).toBeUndefined();
  expect(cache.batches).toEqual([]);
});

test("a mouse dragged along a row never swipes it", async () => {
  const cache = await listed();
  drag(2, PAST_HALF, { pointerType: "mouse" });
  lift(2);
  expect(row(2).dataset["swiping"]).toBeUndefined();
  expect(cache.batches).toEqual([]);
});

test("a second finger sends a moving row home and runs nothing", async () => {
  const cache = await listed();
  drag(2, PAST_HALF);
  act(() => {
    fireEvent.pointerDown(row(3), at(3, 0, { pointerId: 2, isPrimary: false }));
  });
  lift(2);
  expect(cellOf(2).style.transform).toBe("translateX(0px)");
  expect(cache.batches).toEqual([]);
});

test("a cancel sends a moving row home and runs nothing", async () => {
  const cache = await listed();
  drag(2, PAST_HALF);
  act(() => {
    fireEvent.pointerCancel(row(2), at(2, PAST_HALF));
  });
  lift(2);
  expect(cellOf(2).style.transform).toBe("translateX(0px)");
  expect(cache.batches).toEqual([]);
});

test("a finger that leaves the row across its edge sends it home, though it keeps its first target", async () => {
  const cache = await listed();
  drag(2, PAST_HALF);
  act(() => {
    fireEvent.pointerMove(row(2), at(2, PAST_HALF, { clientY: 2 * ROW_PX - OFF_THE_ROW / 2 }));
  });
  lift(2);
  expect(cellOf(2).style.transform).toBe("translateX(0px)");
  expect(cache.batches).toEqual([]);
});

test("a finger past the row's end keeps the row at its full reach, and a release there commits", async () => {
  const cache = await listed();
  drag(2, ROW_WIDTH_PX);
  expect(cellOf(2).style.transform).toBe(`translateX(${String(ROW_WIDTH_PX)}px)`);
  lift(2);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
});

test("short of half the row goes home with nothing sent, then stands as before", async () => {
  const cache = await listed();
  drag(2, SHORT_OF_HALF);
  expect(cellOf(2).style.transform).toBe(`translateX(${String(SHORT_OF_HALF)}px)`);
  lift(2);
  expect(cellOf(2).style.transform).toBe("translateX(0px)");
  expect(cellOf(2).dataset["phase"]).toBe("home");
  act(() => {
    fireEvent.transitionEnd(cellOf(2));
  });
  expect(row(2).dataset["swiping"]).toBeUndefined();
  expect(groundOf(2)).toBeNull();
  expect(cache.batches).toEqual([]);
});

test("under reduced motion a row short of half stands home at once", async () => {
  await listed();
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce") }));
  drag(2, SHORT_OF_HALF);
  lift(2);
  expect(row(2).dataset["swiping"]).toBeUndefined();
  expect(cellOf(2).style.transform).toBe("");
});

test("while a selection stands a touch never swipes a row", async () => {
  const cache = await listed();
  act(() => {
    row(1).focus();
  });
  command("x");
  drag(2, PAST_HALF);
  lift(2);
  expect(row(2).dataset["swiping"]).toBeUndefined();
  expect(cache.batches).toEqual([]);
});

test("a swipe ends a pending long press, so the row is never picked", async () => {
  await listed();
  vi.useFakeTimers();
  drag(2, A_QUARTER);
  act(() => {
    vi.advanceTimersByTime(LONG_PRESS_MS);
  });
  expect(row(2).getAttribute("aria-selected")).toBeNull();
  expect(row(2).dataset["pressing"]).toBeUndefined();
});

test("toward the line's end the icon shows alone, the word from half the row; a release there archives and opens nothing", async () => {
  const opened = vi.fn<(row: ListRow, focus: OpenFocus) => void>();
  const cache = await listed({ onOpen: opened });
  drag(2, A_QUARTER);
  expect(groundOf(2)).toEqual({ way: "toEnd", word: "" });
  act(() => {
    fireEvent.pointerMove(row(2), at(2, PAST_HALF));
  });
  expect(groundOf(2)).toEqual({ way: "toEnd", word: "Archive" });
  expect(row(2).querySelector("[data-way]")?.getAttribute("aria-hidden")).toBe("true");
  lift(2);
  act(() => {
    fireEvent.click(row(2));
  });
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(2).memberIds);
  expect(cache.batches[0]?.[0]).toMatchObject({
    patch: { [`mailboxIds/${INBOX_ID}`]: null, "mailboxIds/mb-archive": true },
  });
  expect(cellOf(2).style.transform).toBe(`translateX(${String(ROW_WIDTH_PX)}px)`);
  expect(cellOf(2).dataset["phase"]).toBe("away");
  expect(opened).not.toHaveBeenCalled();
});

test("toward the line's start a release past half deletes", async () => {
  const cache = await listed();
  drag(2, -PAST_HALF);
  expect(groundOf(2)).toEqual({ way: "toStart", word: "Delete" });
  lift(2);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.[0]).toMatchObject({ patch: { "mailboxIds/mb-trash": true } });
  expect(cellOf(2).style.transform).toBe(`translateX(-${String(ROW_WIDTH_PX)}px)`);
});

test("in Trash the way toward the start deletes forever and the way toward the end keeps the row in place", async () => {
  const cache = await listed({ role: "trash" });
  drag(1, PAST_HALF);
  expect(cellOf(1).style.transform).toBe("translateX(0px)");
  expect(groundOf(1)).toBeNull();
  lift(1);
  expect(cache.batches).toEqual([]);
  drag(1, -PAST_HALF);
  expect(groundOf(1)).toEqual({ way: "toStart", word: "Delete forever" });
  lift(1);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.every((mutation) => "destroy" in mutation)).toBe(true);
});

test("right to left a finger moving left archives, toward the line's end", async () => {
  const cache = await listed({ locale: "en-XA" });
  drag(2, -PAST_HALF);
  expect(groundOf(2)?.way).toBe("toEnd");
  lift(2);
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.[0]).toMatchObject({ patch: { "mailboxIds/mb-archive": true } });
});

test("a change the cache refuses sends the row home", async () => {
  const cache = await listed();
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  cache.mutateMany = () => Promise.reject(new Error("refused"));
  drag(2, PAST_HALF);
  lift(2);
  await vi.waitFor(() => {
    expect(cellOf(2).style.transform).toBe("translateX(0px)");
  });
});

test("a swipe on the last row leaves no close-up behind: a thread that lands at its place later stands still", async () => {
  const { client, server, cache } = await served();
  drag(SERVED - 1, PAST_HALF);
  lift(SERVED - 1);
  await left(client, SERVED - 1);
  expect(closingMarks()).toEqual([undefined, undefined]);
  // Older than the three, so the new thread lands at the end of the list.
  addMail(server, 1, "z", -(SERVED + 1));
  await cache.poll();
  await left(client, SERVED);
  expect(closingMarks()).toEqual([undefined, undefined, undefined]);
});

test("under reduced motion the rows from a left thread's place stand as they are, with no close-up", async () => {
  const { client } = await served();
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce") }));
  drag(0, PAST_HALF);
  lift(0);
  await left(client, SERVED - 1);
  expect(row(0).dataset["swiping"]).toBeUndefined();
  expect(closingMarks()).toEqual([undefined, undefined]);
});

test("a thread that left takes its swipe along and the rows from its place close up once", async () => {
  const { client } = await served();
  const second = row(1).getAttribute("aria-label");
  drag(0, PAST_HALF);
  lift(0);
  await left(client, SERVED - 1);
  expect(row(0).getAttribute("aria-label")).toBe(second);
  expect(row(0).dataset["swiping"]).toBeUndefined();
  expect(cellOf(0).style.transform).toBe("");
  expect(closingMarks()).toEqual(["true", "true"]);
  // jsdom has no AnimationEvent, so React listens for the prefixed name.
  act(() => {
    fireEvent(row(0), new Event("webkitAnimationEnd", { bubbles: true }));
  });
  expect(closingMarks()).toEqual([undefined, undefined]);
});
