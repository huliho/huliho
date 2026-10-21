// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { act, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { MoveMode } from "../../commands/move-sections";
import { commandsSnapshot } from "../../commands/registry";
import { ToastProvider, Toasts } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { FASTMAIL, INBOX_ID, INBOX_PAGE, MAILBOXES } from "../fixtures";
import { command, mockListBox, renderList, row } from "../list-rig";
import { prefetchMenus } from "../menus-chunk";
import { forgetMoveTargets, moveTargetsOf, rememberTarget, rememberedTarget } from "./move-mode";
import { runOnOpenThread } from "./open-thread";
import { WRITABLE, boxOf, rowAt } from "./writable-rig";

const opened = vi.hoisted(() => ({ modes: [] as MoveMode[] }));

vi.mock("../../commands/command-host", () => ({
  openMoveMode: (mode: MoveMode) => {
    opened.modes.push(mode);
  },
}));

const FACTUREN = "mb-facturen";

function WithToasts({ children }: { children: ReactNode }) {
  return (
    <ToastProvider>
      {children}
      <Toasts />
    </ToastProvider>
  );
}

function lastMode(): MoveMode {
  const mode = opened.modes.at(-1);
  if (mode === undefined) {
    throw new Error("the picker never opened");
  }
  return mode;
}

async function listed(options: { openThreadId?: string } = {}) {
  const cache = fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE });
  await act(async () => {
    await prefetchMenus();
  });
  renderList({
    mailbox: boxOf("inbox"),
    mailboxes: WRITABLE,
    cache,
    wrapper: WithToasts,
    ...options,
  });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((entry) => entry.id === "thread.move")).toBe(true);
  });
  act(() => {
    row(0).focus();
  });
  return cache;
}

function moved(target: ListRow, to: string) {
  return target.memberIds.map((id) => ({
    type: "Email",
    id,
    threadId: target.threadId,
    patch: { [`mailboxIds/${INBOX_ID}`]: null, [`mailboxIds/${to}`]: true },
  }));
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
  opened.modes.length = 0;
});

afterEach(() => {
  vi.restoreAllMocks();
  forgetMoveTargets();
});

test("the picker lists every mailbox a move may enter in the tree's order, a nested one by its path", () => {
  const ids = WRITABLE.filter((box) => !["mb-trash", "mb-junk", INBOX_ID].includes(box.id)).map(
    (box) => box.id,
  );
  expect(moveTargetsOf(ids, WRITABLE, "en")).toEqual([
    { id: "mb-drafts", name: "Drafts", path: [] },
    { id: "mb-sent", name: "Sent", path: [] },
    { id: "mb-archive", name: "Archive", path: [] },
    { id: FACTUREN, name: "Facturen", path: [] },
    { id: "mb-verbouwing", name: "Verbouwing", path: [] },
    { id: "mb-offertes", name: "Offertes", path: ["Verbouwing"] },
  ]);
});

test("a loop in the server's parents ends the path", () => {
  // A folder under a role stands at the top of the tree, so the loop
  // through the inbox leaves it in the picker.
  const inbox = { ...boxOf("inbox"), parentId: FACTUREN };
  const facturen = WRITABLE.find((box) => box.id === FACTUREN);
  if (facturen === undefined) {
    throw new Error("the fixtures hold no Facturen");
  }
  const looped = [inbox, { ...facturen, parentId: INBOX_ID }];
  expect(moveTargetsOf([FACTUREN], looped, "en")).toEqual([
    { id: FACTUREN, name: "Facturen", path: ["Inbox"] },
  ]);
});

test("each account remembers its own last target, and a session's end forgets them all", () => {
  rememberTarget("a1", "x");
  rememberTarget("a2", "y");
  expect([rememberedTarget("a1"), rememberedTarget("a2")]).toEqual(["x", "y"]);
  forgetMoveTargets();
  expect([rememberedTarget("a1"), rememberedTarget("a2")]).toEqual([null, null]);
});

test("a storage that refuses to be read throws nothing at a session's end", () => {
  rememberTarget("a1", "x");
  vi.spyOn(Storage.prototype, "key").mockImplementation(() => {
    throw new Error("denied");
  });
  expect(() => {
    forgetMoveTargets();
  }).not.toThrow();
});

test("m opens the picker without the mailbox shown, Trash and Junk", async () => {
  await listed();
  command("m");
  const mode = lastMode();
  expect(mode.targets.map((target) => target.id)).toEqual([
    "mb-drafts",
    "mb-sent",
    "mb-archive",
    FACTUREN,
    "mb-verbouwing",
    "mb-offertes",
  ]);
  expect(mode.recent).toBeNull();
});

test("a pick moves the cursor's thread, says where and is remembered for the next move", async () => {
  const cache = await listed();
  command("m");
  act(() => {
    lastMode().onPick(FACTUREN);
  });
  await vi.waitFor(() => {
    expect(screen.getAllByText("Moved to Facturen.")).toHaveLength(2);
  });
  expect(cache.batches).toEqual([moved(rowAt(0), FACTUREN)]);
  expect(rememberedTarget(FASTMAIL.id)).toBe(FACTUREN);
  command("m");
  expect(lastMode().recent).toBe(FACTUREN);
});

test("the bar's move takes every pick and counts them in the toast", async () => {
  const cache = await listed();
  command("x");
  act(() => {
    row(2).focus();
  });
  command("x");
  const bar = await screen.findByRole("toolbar");
  act(() => {
    within(bar).getByRole("button", { name: "Move to…" }).click();
  });
  act(() => {
    lastMode().onPick(FACTUREN);
  });
  await vi.waitFor(() => {
    expect(screen.getAllByText("2 conversations moved to Facturen.")).toHaveLength(2);
  });
  expect(cache.batches).toEqual([[...moved(rowAt(0), FACTUREN), ...moved(rowAt(2), FACTUREN)]]);
});

test("the toolbar's move takes the open thread, wherever the focus stands", async () => {
  const cache = await listed({ openThreadId: rowAt(1).threadId });
  act(() => {
    row(3).focus();
  });
  act(() => {
    runOnOpenThread("thread.move");
  });
  act(() => {
    lastMode().onPick("mb-archive");
  });
  await vi.waitFor(() => {
    expect(cache.batches).toEqual([moved(rowAt(1), "mb-archive")]);
  });
});

test("a mailbox no other takes mail from offers no move", async () => {
  renderList({ mailbox: MAILBOXES.find((box) => box.id === INBOX_ID) });
  await vi.waitFor(() => {
    expect(commandsSnapshot().some((entry) => entry.id === "thread.read")).toBe(true);
  });
  expect(commandsSnapshot().some((entry) => entry.id === "thread.move")).toBe(false);
});
