// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { cleanup, fireEvent, screen } from "@testing-library/react";
import { useLayoutEffect } from "react";
import type { ReactNode } from "react";
import { expect, test, vi } from "vitest";

import { FASTMAIL, INBOX_ID, INBOX_PAGE, fixtureCache, pageOf } from "./fixtures";
import {
  ROW_PX,
  command,
  focused,
  inbox,
  mockListBox,
  press,
  renderList,
  row,
  rows,
  withPage,
} from "./list-rig";

const NEW_MAIL = 2;
// The still rows after a syncing list.
const SYNC_EDGE_ROWS = 3;
const COMPACT_ROW_PX = "40px";
const TOUCH_ROW_PX = "56px";
const OFFLINE_SENTENCE = "Offline: showing cached mail. The connection comes back on its own.";
const FIRST_SYNC_SENTENCE = "Syncing this mailbox for the first time.";
const NEW_MAIL_SENTENCE = "2 new messages";

mockListBox();

function syncingInbox() {
  return { ...inbox(), totalEmails: 18_532, syncedEmails: 1240 };
}

// The region for new mail; it carries no role.
function politeRegion(): HTMLElement {
  const region = document.querySelector('[aria-live="polite"]');
  if (!(region instanceof HTMLElement)) {
    throw new Error("no polite region");
  }
  return region;
}

// Whether a record put nodes into `target`.
function addsTo(target: Node): (record: MutationRecord) => boolean {
  return (record) => record.target === target && record.addedNodes.length > 0;
}

test("the rows carry their name, place and state; the grid its count", async () => {
  renderList();
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  expect(grid.getAttribute("aria-rowcount")).toBe(String(INBOX_PAGE.ids.length));
  expect(row(0).getAttribute("aria-label")).toBe(
    "Mireille Dekker, Serverwissel zaterdagnacht, korte onderbreking, 9:41 AM, unread",
  );
  expect(row(0).getAttribute("aria-rowindex")).toBe("1");
  expect(row(0).tabIndex).toBe(0);
  expect(row(0).dataset["unread"]).toBe("true");
  expect(row(2).getAttribute("aria-label")).toBe(
    "Pieter Blom, Offerte badkamerrenovatie, herziene versie, 8:15 AM, 14 messages",
  );
  expect(row(2).tabIndex).toBe(-1);
  expect(row(2).textContent).toContain("14");
  expect(row(9).getAttribute("aria-label")).toBe("Unknown sender, (No subject), May 7");
  expect(row(10).getAttribute("aria-label")).toBe(
    "Marktplaats, Je advertentie verloopt bijna, Oct 14, 2025",
  );
});

test("the arrow keys, Home, End, j and k move the one tab stop", async () => {
  renderList();
  await screen.findByRole("grid");
  row(0).focus();
  press("ArrowDown");
  expect(focused()).toBe(row(1));
  expect(row(1).tabIndex).toBe(0);
  expect(row(0).tabIndex).toBe(-1);
  press("End");
  expect(focused()).toBe(rows().at(-1));
  press("ArrowDown");
  expect(focused()).toBe(rows().at(-1));
  press("Home");
  expect(focused()).toBe(row(0));
  press("ArrowUp");
  expect(focused()).toBe(row(0));
  command("j");
  expect(focused()).toBe(row(1));
  command("k");
  expect(focused()).toBe(row(0));
});

test("the marker shows new mail, the dot key or a click reveals it and a scroll puts it away", async () => {
  const cache = withPage(pageOf(undefined, NEW_MAIL));
  renderList({ cache });
  await screen.findByRole("grid");
  const marker = screen.getByRole("button", { name: "2 new messages" });
  expect(marker.textContent).toContain(".");
  command(".");
  expect(cache.revealed).toEqual([INBOX_ID]);
  fireEvent.click(marker);
  expect(cache.revealed).toEqual([INBOX_ID, INBOX_ID]);
  fireEvent.scroll(screen.getByRole("grid"));
  expect(screen.queryByRole("button", { name: /new messages/ })).toBeNull();
});

test("a reveal from the marker button leaves the focus on the cursor's row", async () => {
  const cache = withPage(pageOf(undefined, NEW_MAIL));
  renderList({ cache });
  await screen.findByRole("grid");
  const marker = screen.getByRole("button", { name: "2 new messages" });
  marker.focus();
  command(".");
  expect(cache.revealed).toEqual([INBOX_ID]);
  expect(focused()).toBe(row(0));
  marker.focus();
  fireEvent.click(marker);
  expect(cache.revealed).toEqual([INBOX_ID, INBOX_ID]);
  expect(focused()).toBe(row(0));
});

test("the offline strip, the error state and the empty word each show in their case", async () => {
  const list = renderList();
  await screen.findByRole("grid");
  // The status regions stand empty before use; the sentence lands in one of them.
  expect(screen.getAllByRole("status").map((region) => region.textContent)).toEqual(["", ""]);
  list.rerender({ online: false });
  expect(screen.getByText(OFFLINE_SENTENCE).getAttribute("role")).toBe("status");
  expect(rows().length).toBeGreaterThan(0);
  cleanup();
  renderList({ cache: withPage(new JmapError("unavailable")) });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Couldn’t load your mail.");
  expect(screen.getByRole("button", { name: "Try again" })).toBeDefined();
  cleanup();
  renderList({ cache: fixtureCache({}) });
  expect(await screen.findByText("nothing here")).toBeDefined();
});

test("a list mounted offline and mid-sync gets each sentence after its region is in the document", async () => {
  const records: MutationRecord[] = [];
  const observer = new MutationObserver((batch) => {
    records.push(...batch);
  });
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  renderList({ mailbox: syncingInbox(), online: false });
  await screen.findByRole("grid");
  records.push(...observer.takeRecords());
  observer.disconnect();
  const regions = screen.getAllByRole("status").filter((region) => region.textContent !== "");
  expect(regions.map((region) => region.textContent)).toEqual([
    OFFLINE_SENTENCE,
    FIRST_SYNC_SENTENCE,
  ]);
  // Each sentence is a mutation of its own region, so the region existed empty before it.
  for (const region of regions) {
    expect(records.some((record) => record.target === region && record.addedNodes.length > 0)).toBe(
      true,
    );
  }
});

test("a list mounted with new mail waiting gets its sentence after the commit that puts its region in", () => {
  const page = pageOf(undefined, NEW_MAIL);
  const client = new QueryClient();
  client.setQueryData(queryKeys.window(FASTMAIL.id, INBOX_ID, 0), page);
  const observer = new MutationObserver(() => undefined);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  // The wrapper's layout effect closes every commit: what it takes was written before the paint.
  const inCommits: MutationRecord[] = [];
  function CommitEnd({ children }: { children: ReactNode }) {
    useLayoutEffect(() => {
      inCommits.push(...observer.takeRecords());
    });
    return children;
  }
  renderList({ cache: withPage(page), client, wrapper: CommitEnd });
  const afterCommits = observer.takeRecords();
  observer.disconnect();
  const region = politeRegion();
  expect(screen.getByRole("button", { name: NEW_MAIL_SENTENCE })).toBeDefined();
  expect(region.textContent).toBe(NEW_MAIL_SENTENCE);
  // The sentence is a mutation of its own region, so the region existed empty before it.
  expect([...inCommits, ...afterCommits].some(addsTo(region))).toBe(true);
  // No commit wrote it: the browser paints the empty region before the sentence lands.
  expect(inCommits.some(addsTo(region))).toBe(false);
});

test("a first sync shows its sentence once and its count beside three still rows", async () => {
  renderList({ mailbox: syncingInbox() });
  await screen.findByRole("grid");
  const foot = screen.getByText(FIRST_SYNC_SENTENCE);
  expect(foot.getAttribute("role")).toBe("status");
  expect(screen.getByText("1,240 of 18,532")).toBeDefined();
  const grid = screen.getByRole("grid");
  expect(grid.getAttribute("aria-rowcount")).toBe(String(INBOX_PAGE.ids.length));
  // The list is three rows taller than its rows; the ones in reach are drawn still.
  const sizer = grid.firstElementChild;
  expect(sizer instanceof HTMLElement && sizer.style.blockSize).toBe(
    `${String((INBOX_PAGE.ids.length + SYNC_EDGE_ROWS) * ROW_PX)}px`,
  );
  expect(grid.querySelectorAll('[aria-hidden="true"] > div').length).toBeGreaterThan(0);
  expect(grid.querySelector(`[data-index="${String(INBOX_PAGE.ids.length)}"]`)).toBeNull();
});

test("the row height follows the density token, under a mounted list too", async () => {
  const declaration = document.createElement("div").style;
  declaration.setProperty("--hhx-row-height", COMPACT_ROW_PX);
  vi.spyOn(window, "getComputedStyle").mockReturnValue(declaration);
  renderList();
  await screen.findByRole("grid");
  expect(row(0).style.blockSize).toBe(COMPACT_ROW_PX);
  expect(row(1).style.transform).toBe("translateY(40px)");
  declaration.setProperty("--hhx-row-height", TOUCH_ROW_PX);
  document.documentElement.dataset["density"] = "touch";
  await vi.waitFor(() => {
    expect(row(1).style.transform).toBe("translateY(56px)");
  });
  expect(row(0).style.blockSize).toBe(TOUCH_ROW_PX);
});
