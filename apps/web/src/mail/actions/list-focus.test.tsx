// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { QueryClient } from "@tanstack/react-query";
import { act, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { MAILBOXES } from "../fixtures";
import {
  command,
  coreCache,
  focused,
  invalidateWindows,
  mockListBox,
  renderList,
  row,
  rows,
  servedInbox,
} from "../list-rig";

function writable(box: Mailbox): Mailbox {
  return { ...box, myRights: { ...box.myRights, mayAddItems: true, mayRemoveItems: true } };
}

const WRITABLE = MAILBOXES.map(writable);
const INBOX = WRITABLE.find((box) => box.role === "inbox");

// A list of `count` threads over the core store, the commands
// registered; archive with `e` at the row `at` holding the focus.
async function archivedAt(count: number, at: number): Promise<void> {
  const client = new QueryClient();
  const cache = coreCache(servedInbox(count, "e", -1));
  renderList({ mailbox: INBOX, mailboxes: WRITABLE, cache, client });
  await vi.waitFor(() => {
    expect(rows()).toHaveLength(count);
  });
  act(() => {
    row(at).focus();
  });
  command("e");
  await vi.waitFor(async () => {
    await invalidateWindows(client);
    expect(screen.queryByRole("grid") === null ? 0 : rows().length).toBeLessThan(count);
  });
}

mockListBox();

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a row archived from the list leaves the focus on the row that takes its place", async () => {
  await archivedAt(3, 1);
  await vi.waitFor(() => {
    expect(focused()).toBe(row(1));
  });
  expect(focused().getAttribute("aria-rowindex")).toBe("2");
});

test("the last row archived hands the focus to the row before it", async () => {
  await archivedAt(3, 2);
  await vi.waitFor(() => {
    expect(focused()).toBe(row(1));
  });
});

test("the only row archived leaves the focus inside the list", async () => {
  await archivedAt(1, 0);
  await vi.waitFor(() => {
    expect(document.activeElement).not.toBe(document.body);
  });
  expect(document.activeElement?.closest("[tabindex]")).not.toBeNull();
});
