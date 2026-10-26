// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListPage, Mailbox, Sender, ThreadDetail } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { mailCache } from "../../cache/client";
import { INBOX_ID, MAILBOXES } from "../fixtures";
import { command, mockShellBox, renderShell } from "../shell-rig";

vi.mock("../../cache/client", async () => {
  const fixtures = await import("../fixtures");
  return {
    mailCache: {
      mailboxes: vi.fn<() => Promise<Mailbox[]>>(() => Promise.resolve(fixtures.MAILBOXES)),
      window: vi.fn<() => Promise<ListPage>>(() => Promise.resolve(fixtures.INBOX_PAGE)),
      thread: vi.fn<() => Promise<ThreadDetail>>(() => Promise.resolve(fixtures.THREAD)),
      reveal: vi.fn<() => Promise<never>>(),
      search: vi.fn<() => Promise<ListPage>>(() => Promise.resolve(fixtures.INBOX_PAGE)),
      senders: vi.fn<() => Promise<Sender[]>>(() => Promise.resolve([])),
    },
    attachCache: vi.fn<() => () => void>(() => () => undefined),
    clearCache: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    pollCache: vi.fn<() => void>(),
  };
});

mockShellBox();

async function focusedRow(): Promise<HTMLElement> {
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  const row = await waitFor(() => {
    const found = grid.querySelector<HTMLElement>('[role="row"][tabindex="0"]');
    expect(found).not.toBeNull();
    return found;
  });
  row?.focus();
  return grid;
}

async function field(): Promise<HTMLElement> {
  return screen.findByRole("combobox", { name: "Search mail" });
}

test("`/` from the list opens the search of the mailbox shown with the focus in the field", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  await focusedRow();
  command("/");
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/search");
  });
  expect(router.state.location.search).toEqual({ box: "mb-inbox" });
  const input = await field();
  await waitFor(() => {
    expect(document.activeElement).toBe(input);
  });
  expect(screen.queryByRole("heading", { level: 1, name: "Inbox" })).toBeNull();
});

test("Escape in the empty field goes back and hands the focus to the list it left", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  await focusedRow();
  command("/");
  const input = await field();
  fireEvent.keyDown(input, { key: "Escape" });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  await waitFor(() => {
    expect(document.activeElement?.closest('[role="grid"]')).not.toBeNull();
  });
});

test("the header's button opens the search and closing hands the focus back to it", async () => {
  const router = renderShell("/mail/acc-1/mb-facturen");
  await screen.findByRole("heading", { level: 1, name: "Facturen" });
  const button = screen.getByRole("button", { name: "Search mail" });
  button.focus();
  fireEvent.click(button);
  const input = await field();
  expect(router.state.location.search).toEqual({ box: "mb-facturen" });
  fireEvent.keyDown(input, { key: "Escape" });
  await waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Search mail" }));
  });
});

test("opened from the tree, closing hands the focus back to the tree's item", async () => {
  renderShell("/mail/acc-1/mb-inbox");
  const item = await screen.findByRole("treeitem", { name: "Inbox, 23 unread" });
  item.focus();
  command("/");
  fireEvent.keyDown(await field(), { key: "Escape" });
  await waitFor(() => {
    expect(document.activeElement).toBe(item);
  });
});

test("closing goes back over the entry the open pushed", async () => {
  const router = renderShell("/mail/acc-1/mb-facturen");
  await screen.findByRole("heading", { level: 1, name: "Facturen" });
  await act(async () => {
    await router.navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId: "acc-1", mailboxId: "mb-inbox" },
    });
  });
  await focusedRow();
  command("/");
  fireEvent.keyDown(await field(), { key: "Escape" });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  act(() => {
    router.history.back();
  });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-facturen");
  });
});

test("from Junk the search keeps to Junk", async () => {
  const router = renderShell("/mail/acc-1/mb-junk");
  await screen.findByRole("heading", { level: 1, name: "Junk" });
  command("/");
  await field();
  expect(router.state.location.search).toEqual({ box: "mb-junk", in: "mb-junk" });
});

test("a run replaces the entry the open pushed, so Back returns to the mailbox", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  await focusedRow();
  command("/");
  const input = await field();
  fireEvent.change(input, { target: { value: "offerte" } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => {
    expect(router.state.location.search).toEqual({ q: "offerte", box: "mb-inbox" });
  });
  act(() => {
    router.history.back();
  });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
});

test("a search reached by its address closes onto the mailbox it names", async () => {
  const router = renderShell("/mail/acc-1/search?q=123&box=mb-facturen");
  const input = await field();
  expect(input).toHaveProperty("value", "123");
  fireEvent.keyDown(input, { key: "Escape" });
  fireEvent.keyDown(input, { key: "Escape" });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-facturen");
  });
});

test("a search reached by its address shows the list's still rows until the tree is in", async () => {
  vi.mocked(mailCache).mailboxes.mockReturnValueOnce(new Promise<Mailbox[]>(() => undefined));
  renderShell("/mail/acc-1/search?q=123&box=mb-facturen");
  const main = await screen.findByRole("main");
  expect(within(main).getByRole("status", { name: "Loading…" })).toBeDefined();
  expect(screen.queryByRole("combobox")).toBeNull();
});

test("`/` outside the field while a search stands puts the focus back in it, after the tree changed too", async () => {
  const client = new QueryClient();
  const router = renderShell("/mail/acc-1/mb-inbox", { client });
  await focusedRow();
  command("/");
  const input = await field();
  fireEvent.change(input, { target: { value: "offerte" } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => {
    expect(router.state.location.search).toEqual({ q: "offerte", box: "mb-inbox" });
  });
  // A poll that counts one more unread hands the shell a new row for the inbox.
  const tree = structuredClone(MAILBOXES);
  const inbox = tree.find((row) => row.id === INBOX_ID);
  if (inbox !== undefined) {
    inbox.unreadEmails += 1;
  }
  act(() => {
    client.setQueryData(queryKeys.mailboxes("acc-1"), tree);
  });
  const item = await screen.findByRole("treeitem", { name: "Inbox, 24 unread" });
  act(() => {
    item.focus();
  });
  command("/");
  await waitFor(() => {
    expect(document.activeElement).toBe(input);
  });
  expect(router.state.location.search).toEqual({ q: "offerte", box: "mb-inbox" });
  expect(input).toHaveProperty("value", "offerte");
  // Closed again, `/` opens the search from the mailbox as before.
  fireEvent.keyDown(input, { key: "Escape" });
  fireEvent.keyDown(input, { key: "Escape" });
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  command("/");
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/search");
  });
});
