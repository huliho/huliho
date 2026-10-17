// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListPage, MailCache, Mailbox, ThreadDetail } from "@huliho/core";
import { act, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "../../cache/coordinator";
import { INBOX_PAGE, MAILBOXES, THREAD_ID } from "../fixtures";
import { command, layout, mockShellBox, renderShell } from "../shell-rig";
import { wantsListFocus } from "../thread-history";

const mailboxes = vi.hoisted(() => vi.fn<(accountId: string) => Promise<Mailbox[]>>());
const mutateMany = vi.hoisted(() => vi.fn<MailCache["mutateMany"]>());
vi.mock("../../cache/client", async () => {
  const fixtures = await import("../fixtures");
  return {
    mailCache: {
      mailboxes,
      window: vi.fn<() => Promise<ListPage>>(() => Promise.resolve(fixtures.INBOX_PAGE)),
      thread: vi.fn<() => Promise<ThreadDetail>>(() => Promise.resolve(fixtures.THREAD)),
      reveal: vi.fn<() => Promise<never>>(),
      mutateMany,
      release: vi.fn<() => Promise<void>>(() => Promise.resolve()),
      drop: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    },
    attachCache: vi.fn<(lease: Lease) => () => void>(() => () => undefined),
    clearCache: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    pollCache: vi.fn<() => void>(),
  };
});

function writable(box: Mailbox): Mailbox {
  return { ...box, myRights: { ...box.myRights, mayAddItems: true, mayRemoveItems: true } };
}

// The tree with every mailbox giving up and taking mail, so archive registers.
const WRITABLE = MAILBOXES.map(writable);
const THREAD_PATH = `/mail/acc-1/mb-inbox/${THREAD_ID}`;
// The fixture thread's subject, which the mocked cache answers for every thread.
const SUBJECT = "Offerte badkamerrenovatie, herziene versie";
// The fixture thread's place in the inbox page and the thread after it.
const AT = INBOX_PAGE.rows.findIndex((row) => row.threadId === THREAD_ID);
const NEXT_PATH = `/mail/acc-1/mb-inbox/${INBOX_PAGE.rows[AT + 1]?.threadId ?? ""}`;

mockShellBox();

beforeEach(() => {
  mailboxes.mockReset();
  mailboxes.mockResolvedValue(WRITABLE);
  mutateMany.mockReset();
  mutateMany.mockResolvedValue({ inverses: [], held: [], threads: [THREAD_ID] });
});

// The shell at the fixture thread with its title focused, with the row
// of that thread in the grid. The title is a level 2 heading beside the
// list and the level 1 heading of a screen.
async function openedThread(level: 1 | 2) {
  const router = renderShell(THREAD_PATH);
  const title = await screen.findByRole("heading", { level, name: SUBJECT });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(title);
  });
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  const row = await vi.waitFor(() => {
    const found = grid.querySelector(`[aria-rowindex="${String(AT + 1)}"]`);
    if (!(found instanceof HTMLElement)) {
      throw new Error("the row is not in yet");
    }
    return found;
  });
  return { router, row };
}

test("e on the row of the open thread opens the next one with the focus left on the list's next row", async () => {
  const { router, row } = await openedThread(2);
  act(() => {
    row.focus();
  });
  command("e");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe(NEXT_PATH);
  });
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe(String(AT + 2));
  });
  // The entry loses its mark once the pane has read it.
  await vi.waitFor(() => {
    expect(wantsListFocus(router.state.location.state)).toBe(false);
  });
  expect(document.activeElement?.getAttribute("aria-rowindex")).toBe(String(AT + 2));
});

test("e from the pane opens the next thread with the focus on its title", async () => {
  const { router } = await openedThread(2);
  command("e");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe(NEXT_PATH);
  });
  await vi.waitFor(() => {
    expect(document.activeElement?.tagName).toBe("H2");
  });
  expect(wantsListFocus(router.state.location.state)).toBe(false);
});

test("on a phone a row still focused under the inert list counts for nothing: the next title takes the focus", async () => {
  layout.wide = false;
  const { router, row } = await openedThread(1);
  act(() => {
    row.focus();
  });
  command("e");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe(NEXT_PATH);
  });
  await vi.waitFor(() => {
    expect(document.activeElement?.tagName).toBe("H1");
  });
  expect(document.activeElement?.textContent).toBe(SUBJECT);
});
