// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { MAILBOXES } from "./fixtures";
import { MailboxTree } from "./mailbox-tree";

interface Options {
  // The address the tree sits at, which names the open mailbox.
  path?: string;
  currentId?: string | undefined;
  showLetters?: boolean;
  onNavigate?: () => void;
}

// The links need a router; a memory one at the tree's own address serves.
async function renderTree({
  path = "/mail/acc-1/mb-inbox",
  currentId = "mb-inbox",
  showLetters = true,
  onNavigate,
}: Options = {}) {
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => (
        <MailboxTree
          locale="en"
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
    "Inboxi23",
    "Draftsd2",
    "Sents",
    "Archivea",
    "Junkj1",
    "Trasht",
    "Facturenf3",
    "Verbouwingv",
    "Offerteso",
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

test("the letters show only where asked for", async () => {
  await renderTree({ showLetters: false });
  expect(item("Verbouwing").textContent).toBe("Verbouwing");
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
