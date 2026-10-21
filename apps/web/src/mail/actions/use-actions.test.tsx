// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, MailCache, Mailbox } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, expect, test, vi } from "vitest";

import { commandsSnapshot } from "../../commands/registry";
import { ToastProvider, Toasts, toastManager } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { INBOX_ID, INBOX_PAGE, MAILBOXES } from "../fixtures";
import { command, focused, mockListBox, renderList, row, settle } from "../list-rig";
import type { Options } from "../list-rig";
import type { OpenFocus } from "../thread-history";
import { runOnOpenThread } from "./open-thread";

type Opened = (row: ListRow, focus: OpenFocus) => void;

const WRITABLE: Mailbox[] = MAILBOXES.map((box) => ({
  ...box,
  myRights: { ...box.myRights, mayAddItems: true, mayRemoveItems: true },
}));

function shown(role: string): Mailbox {
  const found = WRITABLE.find((box) => box.role === role);
  if (found === undefined) {
    throw new Error(`no ${role} in the fixtures`);
  }
  return found;
}

function first(): ListRow {
  const found = INBOX_PAGE.rows[0];
  if (found === undefined) {
    throw new Error("the fixture page holds no row");
  }
  return found;
}

function WithToasts({ children }: { children: ReactNode }) {
  return (
    <ToastProvider>
      {children}
      <Toasts />
    </ToastProvider>
  );
}

// The Act group as the registry holds it: each command's label and key.
function acts(): string[] {
  return commandsSnapshot()
    .filter((entry) => entry.id.startsWith("thread."))
    .map((entry) => `${entry.label} ${entry.keys[0]?.key ?? ""}`);
}

// The Act group once the list registered it.
async function registered(): Promise<string[]> {
  await vi.waitFor(() => {
    expect(acts().length).toBeGreaterThan(0);
  });
  return acts();
}

// The labels of the two flips, whichever order they registered in.
function flips(): string[] {
  return ["thread.read", "thread.star"].map(
    (id) => commandsSnapshot().findLast((entry) => entry.id === id)?.label ?? "",
  );
}

// A layered surface with a button, as the palette holds the focus.
function layer(): { inside: HTMLButtonElement; remove: () => void } {
  const surface = document.createElement("div");
  surface.setAttribute("role", "dialog");
  const inside = document.createElement("button");
  surface.append(inside);
  document.body.append(surface);
  return {
    inside,
    remove: () => {
      surface.remove();
    },
  };
}

function client(afterAction: "next" | "list"): QueryClient {
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.preferences, { afterAction });
  return queryClient;
}

// A list of `role` over the fixture page, the commands registered and
// the focus on its first row.
async function listed(role: string, options: Options = {}) {
  const cache = fixtureCache({ [`${shown(role).id}/0`]: INBOX_PAGE });
  renderList({ mailbox: shown(role), mailboxes: WRITABLE, cache, wrapper: WithToasts, ...options });
  await registered();
  act(() => {
    row(0).focus();
  });
  return cache;
}

mockListBox();

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => undefined));
});

test("a mailbox that gives up no email registers the marks alone", async () => {
  renderList({ mailbox: MAILBOXES.find((box) => box.id === INBOX_ID) });
  expect(await registered()).toEqual(["Mark as read u", "Add star s"]);
});

test("the inbox registers archive, delete, spam, the two flips and move with their keys", async () => {
  renderList({ mailbox: shown("inbox"), mailboxes: WRITABLE });
  expect(await registered()).toEqual([
    "Archive e",
    "Delete #",
    "Mark as spam !",
    `${first().unread ? "Mark as read" : "Mark as unread"} u`,
    `${first().flagged ? "Remove star" : "Add star"} s`,
    "Move to… m",
  ]);
});

test("Trash offers delete forever and spam, Junk delete forever and not spam, neither archive", async () => {
  const cache = fixtureCache({ "mb-trash/0": INBOX_PAGE, "mb-junk/0": INBOX_PAGE });
  const trash = renderList({ mailbox: shown("trash"), mailboxes: WRITABLE, cache });
  expect((await registered()).slice(0, 2)).toEqual(["Delete forever #", "Mark as spam !"]);
  trash.rerender({ mailbox: shown("junk") });
  await vi.waitFor(() => {
    expect(acts().slice(0, 2)).toEqual(["Delete forever #", "Not spam !"]);
  });
});

test("the flips name the row in focus while a thread is open, through a layer and back", async () => {
  // The open thread's row is unread without a star; the third row is read and starred.
  renderList({ mailbox: shown("inbox"), mailboxes: WRITABLE, openThreadId: first().threadId });
  await registered();
  expect(flips()).toEqual(["Mark as read", "Add star"]);
  act(() => {
    row(2).focus();
  });
  await vi.waitFor(() => {
    expect(flips()).toEqual(["Mark as unread", "Remove star"]);
  });
  // A layered surface takes the focus and gives it back before a pick runs.
  const surface = layer();
  act(() => {
    surface.inside.focus();
  });
  await settle();
  expect(flips()).toEqual(["Mark as unread", "Remove star"]);
  act(() => {
    row(2).focus();
    row(2).blur();
  });
  await vi.waitFor(() => {
    expect(flips()).toEqual(["Mark as read", "Add star"]);
  });
  surface.remove();
});

test("e archives every email of the cursor's thread, says so twice and Undo sends the inverses", async () => {
  const cache = await listed("inbox");
  command("e");
  // The toast and the list's status region.
  await vi.waitFor(() => {
    expect(screen.getAllByText("Archived.")).toHaveLength(2);
  });
  expect(cache.batches).toEqual([
    first().memberIds.map((id) => ({
      type: "Email",
      id,
      threadId: first().threadId,
      patch: { [`mailboxIds/${INBOX_ID}`]: null, "mailboxIds/mb-archive": true },
    })),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(
    cache.batches[1]?.map((mutation) => ("patch" in mutation ? mutation.patch : null)),
  ).toEqual(
    first().memberIds.map(() => ({
      [`mailboxIds/${INBOX_ID}`]: true,
      "mailboxIds/mb-archive": null,
    })),
  );
});

test("star and the read flip change in place without a toast", async () => {
  const cache = await listed("inbox");
  command("s");
  command("u");
  await vi.waitFor(() => {
    expect(cache.batches).toHaveLength(2);
  });
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

test("delete forever holds its destroys: undo drops them and the toast's end releases them", async () => {
  const cache = await listed("trash");
  command("#");
  await screen.findAllByText("Deleted forever.");
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(cache.dropped).toEqual([[1]]);
  expect(cache.released).toEqual([]);
  command("#");
  await screen.findAllByText("Deleted forever.");
  act(() => {
    toastManager.close();
  });
  await vi.waitFor(() => {
    expect(cache.released).toEqual([[2]]);
  });
});

test("an action on the open thread opens the next row under next, with the focus on its title", async () => {
  const opened = vi.fn<Opened>();
  const closed = vi.fn<() => void>();
  const open = { openThreadId: first().threadId, onOpen: opened, onClose: closed };
  renderList({ mailbox: shown("inbox"), mailboxes: WRITABLE, client: client("next"), ...open });
  await registered();
  command("e");
  await vi.waitFor(() => {
    expect(opened).toHaveBeenCalledExactlyOnceWith(INBOX_PAGE.rows[1], "title");
  });
  expect(closed).not.toHaveBeenCalled();
});

test("e on the row of the open thread opens the next one and leaves the focus on its row", async () => {
  const opened = vi.fn<Opened>();
  const open = { openThreadId: first().threadId, onOpen: opened };
  renderList({ mailbox: shown("inbox"), mailboxes: WRITABLE, client: client("next"), ...open });
  await registered();
  act(() => {
    row(0).focus();
  });
  command("e");
  await vi.waitFor(() => {
    expect(opened).toHaveBeenCalledExactlyOnceWith(INBOX_PAGE.rows[1], "list");
  });
  await vi.waitFor(() => {
    expect(focused()).toBe(row(1));
  });
});

test("a change the log took nothing of leaves the open thread as it is and says nothing", async () => {
  const opened = vi.fn<Opened>();
  const closed = vi.fn<() => void>();
  const mutateMany = vi.fn<MailCache["mutateMany"]>(() =>
    Promise.resolve({ inverses: [], held: [], threads: [] }),
  );
  const cache = { ...fixtureCache({ [`${shown("inbox").id}/0`]: INBOX_PAGE }), mutateMany };
  renderList({
    mailbox: shown("inbox"),
    mailboxes: WRITABLE,
    cache,
    client: client("next"),
    openThreadId: first().threadId,
    onOpen: opened,
    onClose: closed,
    wrapper: WithToasts,
  });
  await registered();
  command("e");
  await vi.waitFor(() => {
    expect(mutateMany).toHaveBeenCalledOnce();
  });
  await settle();
  expect(opened).not.toHaveBeenCalled();
  expect(closed).not.toHaveBeenCalled();
  expect(screen.queryByText("Archived.")).toBeNull();
});

test("under list, and at the end of the list under next, the thread closes", async () => {
  const opened = vi.fn<Opened>();
  const closed = vi.fn<() => void>();
  const open = { openThreadId: first().threadId, onOpen: opened, onClose: closed };
  renderList({ mailbox: shown("inbox"), mailboxes: WRITABLE, client: client("list"), ...open });
  await registered();
  command("e");
  await vi.waitFor(() => {
    expect(closed).toHaveBeenCalledOnce();
  });
  cleanup();
  const last = INBOX_PAGE.rows.at(-1)?.threadId ?? null;
  renderList({
    mailbox: shown("inbox"),
    mailboxes: WRITABLE,
    client: client("next"),
    ...open,
    openThreadId: last,
  });
  await registered();
  command("e");
  await vi.waitFor(() => {
    expect(closed).toHaveBeenCalledTimes(2);
  });
  expect(opened).not.toHaveBeenCalled();
});

test("a press in the toolbar takes the open thread whatever row the focus stands on and moves on to its title", async () => {
  const opened = vi.fn<Opened>();
  const open = { openThreadId: first().threadId, onOpen: opened };
  const cache = fixtureCache({ [`${shown("inbox").id}/0`]: INBOX_PAGE });
  renderList({
    mailbox: shown("inbox"),
    mailboxes: WRITABLE,
    cache,
    client: client("next"),
    ...open,
  });
  await registered();
  act(() => {
    row(2).focus();
  });
  act(() => {
    runOnOpenThread("thread.archive");
  });
  await vi.waitFor(() => {
    expect(opened).toHaveBeenCalledExactlyOnceWith(INBOX_PAGE.rows[1], "title");
  });
  expect(cache.batches[0]?.map((mutation) => mutation.id)).toEqual(first().memberIds);
  cleanup();
  // Without a list nothing lends the toolbar a thread to act on.
  runOnOpenThread("thread.archive");
  expect(cache.batches).toHaveLength(1);
});
