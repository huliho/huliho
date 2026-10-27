// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { searchKeyOf } from "@huliho/core";
import type { ListPage, SearchParams } from "@huliho/core";
import { act, fireEvent, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, expect, test, vi } from "vitest";

import type { MoveMode } from "../../commands/move-sections";
import { commandsSnapshot } from "../../commands/registry";
import { ToastProvider, Toasts } from "../../design-system/toast";
import { runOnOpenThread } from "../actions/open-thread";
import { swipeActionsOf } from "../actions/swipe-row";
import { placesOf } from "../actions/taken";
import { aimAt } from "../actions/targets";
import { WRITABLE, rowAt } from "../actions/writable-rig";
import { fixtureCache } from "../fixture-cache";
import type { ThreadAnswer } from "../fixture-cache";
import { FASTMAIL, INBOX_ID, THREAD, THREAD_ID } from "../fixtures";
import { command, mockListBox, renderList, row } from "../list-rig";
import type { Options } from "../list-rig";
import { prefetchMenus } from "../menus-chunk";
import { resultsSource } from "./search-results";

const opened = vi.hoisted(() => ({ modes: [] as MoveMode[] }));

vi.mock("../../commands/command-host", () => ({
  openMoveMode: (mode: MoveMode) => {
    opened.modes.push(mode);
  },
}));

const ARCHIVE = "mb-archive";
const TRASH = "mb-trash";
// Every mailbox but Junk and Trash; every mailbox; one folder.
const SPREAD: SearchParams = { q: "offerte" };
const ANYWHERE: SearchParams = { q: "offerte", in: "all" };
const ONE: SearchParams = { q: "offerte", in: "mb-facturen" };

function WithToasts({ children }: { children: ReactNode }) {
  return (
    <ToastProvider>
      {children}
      <Toasts />
    </ToastProvider>
  );
}

// The fixture page's first rows, each acting from the mailbox given.
function found(mailboxIds: readonly string[]): ListPage {
  const rows = mailboxIds.map((mailboxId, index) => ({ ...rowAt(index), mailboxId }));
  return { rows, total: rows.length, pending: 0 };
}

interface Listed extends Pick<Options, "onOpen" | "onClose"> {
  actsFrom?: string;
  openThreadId?: string;
  threads?: Record<string, ThreadAnswer>;
}

// The results of `params` as the list draws them over the writable tree,
// with the bar's chunk in.
async function listed(params: SearchParams, mailboxIds: readonly string[], more: Listed = {}) {
  const key = searchKeyOf(params);
  const cache = fixtureCache({ [`search:${key}/0`]: found(mailboxIds) }, more.threads ?? {});
  await act(async () => {
    await prefetchMenus();
  });
  const context = {
    cache,
    accountId: FASTMAIL.id,
    locale: "en",
    mailboxes: WRITABLE,
    result: { actsFrom: more.actsFrom ?? null },
  };
  renderList({
    mailboxes: WRITABLE,
    cache,
    wrapper: WithToasts,
    openThreadId: more.openThreadId ?? null,
    source: (held) => resultsSource({ ...context, cache: held }, key, params),
    ...(more.onOpen === undefined ? {} : { onOpen: more.onOpen }),
    ...(more.onClose === undefined ? {} : { onClose: more.onClose }),
  });
  await screen.findByRole("grid");
  return cache;
}

function lastMode(): MoveMode {
  const mode = opened.modes.at(-1);
  if (mode === undefined) {
    throw new Error("the picker never opened");
  }
  return mode;
}

function hover(index: number): void {
  act(() => {
    fireEvent.pointerOver(row(index), { pointerType: "mouse" });
  });
}

// The labels of a row's actions.
function actionsOf(index: number): (string | null)[] {
  return within(row(index))
    .queryAllByRole("button")
    .map((button) => button.getAttribute("aria-label"));
}

function registered(id: string): boolean {
  return commandsSnapshot().some((entry) => entry.id === id);
}

// Picks the rows at `indexes` with the selection's key.
async function pick(indexes: readonly number[]): Promise<void> {
  for (const index of indexes) {
    act(() => {
      row(index).focus();
    });
    command("x");
  }
  await screen.findByRole("toolbar");
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
  opened.modes.length = 0;
});

test("a result names the mailbox it acts from where the search spans several, its name too", async () => {
  await listed(SPREAD, [INBOX_ID, ARCHIVE]);
  expect(within(row(0)).getByText("Inbox")).toBeDefined();
  expect(within(row(1)).getByText("Archive")).toBeDefined();
  expect(row(1).getAttribute("aria-label")).toMatch(/, in Archive$/u);
});

test("a search of one mailbox names it on no row", async () => {
  await listed(ONE, ["mb-facturen", "mb-facturen"]);
  expect(within(row(0)).queryByText("Facturen")).toBeNull();
  expect(row(0).getAttribute("aria-label")).not.toMatch(/, in /u);
});

test("a result's strip holds what its own mailbox offers, so an archived thread has no archive", async () => {
  await listed(SPREAD, [INBOX_ID, ARCHIVE]);
  hover(0);
  expect(actionsOf(0)[0]).toBe("Archive");
  expect(actionsOf(0)).toHaveLength(4);
  hover(1);
  expect(actionsOf(1)).not.toContain("Archive");
  expect(actionsOf(1)).toHaveLength(3);
});

test("a key follows the cursor's result: archive stands on an inbox thread and leaves on an archived one", async () => {
  await listed(SPREAD, [INBOX_ID, ARCHIVE]);
  act(() => {
    row(0).focus();
  });
  await vi.waitFor(() => {
    expect(registered("thread.archive")).toBe(true);
  });
  act(() => {
    row(1).focus();
  });
  await vi.waitFor(() => {
    expect(registered("thread.archive")).toBe(false);
  });
  expect(registered("thread.delete")).toBe(true);
});

test("a bulk archive on results from two mailboxes moves the inbox thread alone and counts it", async () => {
  const cache = await listed(SPREAD, [INBOX_ID, ARCHIVE]);
  await pick([0, 1]);
  expect(
    within(screen.getByRole("toolbar")).getByRole("button", { name: "Archive" }),
  ).toBeDefined();
  command("e");
  await vi.waitFor(() => {
    expect(screen.getAllByText("Archived.")).toHaveLength(2);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(0).memberIds);
});

test("a move of picks from the inbox and the archive to the archive changes the inbox pick alone; the archived one keeps the cursor and stays open", async () => {
  const onOpen = vi.fn<NonNullable<Options["onOpen"]>>();
  const onClose = vi.fn<NonNullable<Options["onClose"]>>();
  const cache = await listed(SPREAD, [INBOX_ID, ARCHIVE, INBOX_ID], {
    openThreadId: rowAt(1).threadId,
    onOpen,
    onClose,
  });
  await pick([0, 1]);
  act(() => {
    within(screen.getByRole("toolbar")).getByRole("button", { name: "Move to…" }).click();
  });
  act(() => {
    lastMode().onPick(ARCHIVE);
  });
  await vi.waitFor(() => {
    expect(screen.getAllByText("Moved to Archive.")).toHaveLength(2);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(0).memberIds);
  expect(row(1).getAttribute("tabindex")).toBe("0");
  expect(onOpen).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});

test("delete on results from the inbox and Trash moves the inbox thread and destroys nothing", async () => {
  const cache = await listed(ANYWHERE, [INBOX_ID, TRASH]);
  await pick([0, 1]);
  command("#");
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(rowAt(0).memberIds);
  expect(cache.batches[0]?.some((mutation) => "destroy" in mutation)).toBe(false);
});

test("results in Trash alone are deleted forever, held until the toast ends", async () => {
  const cache = await listed(ANYWHERE, [TRASH, TRASH]);
  await pick([0, 1]);
  command("#");
  await screen.findAllByText("2 conversations deleted forever.");
  expect(cache.batches[0]?.every((mutation) => "destroy" in mutation)).toBe(true);
});

test("the open result no page holds acts from the mailbox its address names", async () => {
  const cache = await listed(SPREAD, [ARCHIVE], {
    actsFrom: INBOX_ID,
    openThreadId: THREAD_ID,
    threads: { [THREAD_ID]: THREAD },
  });
  await vi.waitFor(() => {
    expect(registered("thread.archive")).toBe(true);
  });
  act(() => {
    runOnOpenThread("thread.archive");
  });
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(1);
  });
  const members = Object.entries(THREAD.thread.members)
    .filter(([, member]) => INBOX_ID in member.mailboxIds)
    .map(([id]) => id);
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(members);
});

test("an archived result swipes toward delete alone", () => {
  const placeOf = placesOf(WRITABLE);
  const archived = aimAt({ ...rowAt(0), mailboxId: ARCHIVE }, 0, placeOf);
  const inbox = aimAt(rowAt(0), 0, placeOf);
  expect([...swipeActionsOf(archived, "en", false).keys()]).toEqual(["toStart"]);
  expect([...swipeActionsOf(inbox, "en", false).keys()]).toEqual(["toEnd", "toStart"]);
  expect(swipeActionsOf(inbox, "en", true).size).toBe(0);
});
