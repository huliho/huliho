// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import type { MailCache } from "@huliho/core";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { LONG_PRESS_MS } from "./actions/long-press";
import { fixtureCache } from "./fixture-cache";
import { MAILBOXES } from "./fixtures";
import { MailboxTree } from "./mailbox-tree";
import { prefetchMenus } from "./menus-chunk";

interface Options {
  // The address the tree sits at, which names the open mailbox.
  path?: string;
  currentId?: string | undefined;
  showLetters?: boolean;
  onNavigate?: () => void;
  cache?: MailCache;
}

// The links need a router; a memory one at the tree's own address serves.
async function renderTree({
  path = "/mail/acc-1/mb-inbox",
  currentId = "mb-inbox",
  showLetters = true,
  onNavigate,
  cache = fixtureCache({}),
}: Options = {}) {
  await act(async () => {
    await prefetchMenus();
  });
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => (
        <MailboxTree
          locale="en"
          cache={cache}
          accountId="acc-1"
          mailboxes={MAILBOXES}
          currentId={currentId}
          showLetters={showLetters}
          onNavigate={onNavigate}
        />
      ),
    }),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(<RouterProvider router={router} />);
  return screen.findByRole("tree", { name: "Mailboxes" });
}

function item(name: string | RegExp): HTMLElement {
  return screen.getByRole("treeitem", { name });
}

afterEach(cleanup);

test("the roles come first in their order, then the folders with their depth", async () => {
  const tree = await renderTree();
  expect(
    within(tree)
      .getAllByRole("treeitem")
      .map((row) => row.textContent),
  ).toEqual([
    "Inbox 23",
    "Drafts 2",
    "Sent",
    "Archive",
    "Junk 1",
    "Trash",
    "Facturen 3",
    "Verbouwing",
    "Offertes",
  ]);
  expect(item("Offertes").getAttribute("aria-level")).toBe("2");
  expect(item("Offertes").getAttribute("aria-posinset")).toBe("1");
  expect(item("Verbouwing").getAttribute("aria-setsize")).toBe("2");
  expect(within(tree).getByText("Folders")).toBeDefined();
  expect(item("Inbox, 23 unread").getAttribute("href")).toBe("/mail/acc-1/mb-inbox");
});

test("the open mailbox is current and the one tab stop; without one the first row is", async () => {
  await renderTree();
  expect(item("Inbox, 23 unread").getAttribute("aria-current")).toBe("page");
  expect(item("Inbox, 23 unread").tabIndex).toBe(0);
  expect(item("Drafts, 2 drafts").tabIndex).toBe(-1);
  cleanup();
  await renderTree({ path: "/mail/acc-1", currentId: undefined });
  expect(item("Inbox, 23 unread").getAttribute("aria-current")).toBeNull();
  expect(item("Inbox, 23 unread").tabIndex).toBe(0);
});

test("each letter stands beside its row's link, and only where asked for", async () => {
  const tree = await renderTree();
  expect(
    within(tree)
      .getAllByRole("treeitem")
      .map((row) => row.nextElementSibling?.textContent),
  ).toEqual(["i", "d", "s", "a", "j", "t", "f", "v", "o"]);
  cleanup();
  await renderTree({ showLetters: false });
  expect(item("Verbouwing").nextElementSibling).toBeNull();
});

test("the arrow keys, Home and End move focus inside the tree", async () => {
  await renderTree();
  const inbox = item("Inbox, 23 unread");
  inbox.focus();
  fireEvent.keyDown(inbox, { key: "ArrowDown" });
  expect(document.activeElement).toBe(item("Drafts, 2 drafts"));
  fireEvent.keyDown(item("Drafts, 2 drafts"), { key: "ArrowUp" });
  expect(document.activeElement).toBe(inbox);
  fireEvent.keyDown(inbox, { key: "ArrowUp" });
  expect(document.activeElement).toBe(inbox);
  fireEvent.keyDown(inbox, { key: "End" });
  expect(document.activeElement).toBe(item("Offertes"));
  fireEvent.keyDown(item("Offertes"), { key: "Home" });
  expect(document.activeElement).toBe(inbox);
});

test("picking a row tells the caller, so a sheet can close", async () => {
  const onNavigate = vi.fn<() => void>();
  await renderTree({ onNavigate });
  fireEvent.click(item("Facturen, 3 unread"));
  expect(onNavigate).toHaveBeenCalledOnce();
});

test("a right click opens the mailbox's menu, whose item runs the job; an item that offers nothing keeps the browser's menu", async () => {
  const cache = fixtureCache({});
  await renderTree({ cache });
  const opened = fireEvent.contextMenu(item("Facturen, 3 unread"), { clientX: 40, clientY: 90 });
  expect(opened).toBe(false);
  fireEvent.click(await screen.findByRole("menuitem", { name: "Mark all as read" }));
  await vi.waitFor(() => {
    expect(cache.jobs).toEqual([{ kind: "markRead", mailboxId: "mb-facturen" }]);
  });
  expect(fireEvent.contextMenu(item("Sent"))).toBe(true);
  expect(screen.queryByRole("menu")).toBeNull();
});

test("the context-menu key opens the menu below the item and Escape gives the item the focus back", async () => {
  await renderTree();
  const inbox = item("Inbox, 23 unread");
  inbox.focus();
  fireEvent.keyDown(inbox, { key: "ContextMenu" });
  const menu = await screen.findByRole("menu");
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((entry) => entry.textContent),
  ).toEqual(["Mark all as read"]);
  fireEvent.keyDown(menu, { key: "Escape" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(inbox);
  });
  fireEvent.keyDown(inbox, { key: "F10", shiftKey: true });
  expect(await screen.findByRole("menu")).toBeDefined();
});

test("a long press opens the menu and the click it leaves behind goes nowhere", async () => {
  const onNavigate = vi.fn<() => void>();
  await renderTree({ onNavigate });
  vi.useFakeTimers();
  try {
    const junk = item("Junk, 1 unread");
    fireEvent.pointerDown(junk, {
      pointerType: "touch",
      isPrimary: true,
      clientX: 30,
      clientY: 200,
    });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    fireEvent.pointerUp(junk, { pointerType: "touch" });
    fireEvent.click(junk);
    expect(onNavigate).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
  expect(await screen.findByRole("menuitem", { name: "Mark all as read" })).toBeDefined();
});
