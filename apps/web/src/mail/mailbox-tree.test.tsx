// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import type { MailCache, Mailbox } from "@huliho/core";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useEffect, useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { closeFolderAsk, settleFolderAsk } from "./actions/folder-asks";
import { FolderHost } from "./actions/folder-host";
import type { FolderShell } from "./actions/folder-items";
import { LONG_PRESS_MS } from "./actions/long-press";
import { MANAGED } from "./actions/writable-rig";
import { StillObserver } from "./card-rig";
import { fixtureCache } from "./fixture-cache";
import { FOLDER_LIMITS, MAILBOXES } from "./fixtures";
import { MailboxTree } from "./mailbox-tree";
import { prefetchMenus } from "./menus-chunk";

interface Options {
  // The address the tree sits at, which names the open mailbox.
  path?: string;
  currentId?: string | undefined;
  showLetters?: boolean;
  onNavigate?: () => void;
  cache?: MailCache;
  mailboxes?: readonly Mailbox[];
  folders?: FolderShell | null;
}

// Hands the tree another list, as a poll does.
let showRows: (rows: readonly Mailbox[]) => void = () => undefined;

function Rig({
  rows,
  ...props
}: Omit<Options, "path" | "mailboxes"> & { rows: readonly Mailbox[] }) {
  const [mailboxes, setMailboxes] = useState(rows);
  useEffect(() => {
    showRows = setMailboxes;
  }, []);
  return (
    <>
      <MailboxTree
        locale="en"
        cache={props.cache ?? fixtureCache({})}
        accountId="acc-1"
        mailboxes={mailboxes}
        currentId={props.currentId}
        showLetters={props.showLetters ?? true}
        folders={props.folders ?? null}
        onNavigate={props.onNavigate}
      />
      <FolderHost locale="en" />
    </>
  );
}

// The links need a router; a memory one at the tree's own address serves.
async function renderTree({
  path = "/mail/acc-1/mb-inbox",
  currentId = "mb-inbox",
  mailboxes = MAILBOXES,
  ...props
}: Options = {}) {
  await act(async () => {
    await prefetchMenus();
  });
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => <Rig rows={mailboxes} currentId={currentId} {...props} />,
    }),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(<RouterProvider router={router} />);
  return screen.findByRole("tree", { name: "Mailboxes" });
}

function item(name: string | RegExp): HTMLElement {
  return screen.getByRole("treeitem", { name });
}

// jsdom has no ResizeObserver; the tree watches its rows with one.
beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StillObserver);
});

// A dialog the test asked for leaves with it.
function dropAsk(): void {
  act(() => {
    closeFolderAsk();
    settleFolderAsk();
  });
}

afterEach(() => {
  dropAsk();
  cleanup();
  vi.unstubAllGlobals();
});

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

// The fixtures' tree with the folders the ids name unsubscribed.
function hiding(...ids: string[]): Mailbox[] {
  return MAILBOXES.map((row) => (ids.includes(row.id) ? { ...row, isSubscribed: false } : row));
}

const FOLDERS: FolderShell = { limits: FOLDER_LIMITS, leave: () => undefined };

test("the tree draws the subscribed folders and the roles, a parent above a subscribed folder too", async () => {
  const tree = await renderTree({ mailboxes: hiding("mb-facturen", "mb-verbouwing") });
  expect(
    within(tree)
      .getAllByRole("treeitem")
      .slice(-2)
      .map((row) => row.textContent),
  ).toEqual(["Verbouwing", "Offertes"]);
  expect(screen.queryByRole("treeitem", { name: /^Facturen/ })).toBeNull();
  cleanup();
  const none = await renderTree({
    mailboxes: MAILBOXES.map((row) => ({ ...row, isSubscribed: false })),
  });
  expect(within(none).getAllByRole("treeitem")).toHaveLength(MAILBOXES.length);
});

test("New folder stands outside the tree at the caption and asks for a folder at the top", async () => {
  const tree = await renderTree({ folders: FOLDERS });
  const button = screen.getByRole("button", { name: "New folder" });
  expect(tree.contains(button)).toBe(false);
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", { name: "New folder" });
  expect(dialog.textContent).toContain("At the top level");
  dropAsk();
  cleanup();
  await renderTree({
    folders: { ...FOLDERS, limits: { ...FOLDER_LIMITS, mayCreateTopLevelMailbox: false } },
  });
  expect(screen.queryByRole("button", { name: "New folder" })).toBeNull();
  cleanup();
  await renderTree();
  expect(screen.queryByRole("button", { name: "New folder" })).toBeNull();
});

test("an item that leaves while it holds the focus hands it to the tree's tab stop", async () => {
  await renderTree({ path: "/mail/acc-1/mb-inbox" });
  item("Offertes").focus();
  act(() => {
    showRows(MAILBOXES.filter((row) => row.id !== "mb-offertes"));
  });
  expect(document.activeElement).toBe(item("Inbox, 23 unread"));
});

test("a folder's menu holds the folder items after its jobs, the delete in danger; a hide of the mailbox shown leaves it", async () => {
  const leave = vi.fn<() => void>();
  await renderTree({
    path: "/mail/acc-1/mb-facturen",
    currentId: "mb-facturen",
    mailboxes: MANAGED,
    folders: { ...FOLDERS, leave },
  });
  fireEvent.contextMenu(item("Facturen, 3 unread"));
  const menu = await screen.findByRole("menu");
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((entry) => entry.textContent),
  ).toEqual(["Mark all as read", "New subfolder", "Rename", "Hide", "Delete"]);
  expect(within(menu).getByRole("menuitem", { name: "Delete" }).dataset["danger"]).toBe("true");
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Hide" }));
  await vi.waitFor(() => {
    expect(leave).toHaveBeenCalledOnce();
  });
});
