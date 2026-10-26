// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, MailCache, Mailbox } from "@huliho/core";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { commandsSnapshot, installCommandListener, registerCommand } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { ConfirmEmpty, askEmpty } from "./actions/confirm-empty";
import { JOB_LAPSE_MS, heardJob } from "./actions/job-store";
import { lendSelection } from "./actions/selection-lend";
import { WRITABLE, boxOf } from "./actions/writable-rig";
import { fixtureCache } from "./fixture-cache";
import { FASTMAIL, INBOX_ID, MAILBOXES } from "./fixtures";
import { prefetchMenus } from "./menus-chunk";
import { PaneHeader } from "./pane-header";

const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
const NAME = INBOX?.name ?? "";

function nothing(): void {
  // The test's x toggles nothing.
}

// The list's key that picks a row, as a list that lets threads be picked registers it.
function pickable(): () => void {
  return registerCommand({
    id: "selection.toggle",
    label: "Select",
    group: "act",
    keys: [{ key: "x" }],
    run: nothing,
  });
}

function progress(state: JobProgress["state"], done = 500): JobProgress {
  return { job: { kind: "empty", mailboxId: "mb-trash" }, done, total: 1204, last: null, state };
}

// The header on a tree, with the dialog a job may ask.
function pane(cache: MailCache, mailbox: Mailbox | undefined, mailboxes: readonly Mailbox[]) {
  return (
    <>
      <PaneHeader
        locale="en"
        cache={cache}
        account={FASTMAIL}
        mailbox={mailbox}
        mailboxes={mailboxes}
        pending={false}
      />
      <ConfirmEmpty locale="en" />
    </>
  );
}

async function header(mailbox: Mailbox | undefined = INBOX) {
  const cache = fixtureCache({});
  await act(async () => {
    await prefetchMenus();
  });
  render(pane(cache, mailbox, WRITABLE));
  return cache;
}

async function menuItems(name: string): Promise<string[]> {
  fireEvent.click(await screen.findByRole("button", { name }));
  return (await screen.findAllByRole("menuitem")).map((item) => item.textContent);
}

afterEach(() => {
  act(() => {
    heardJob(FASTMAIL.id, progress("done"));
    window.dispatchEvent(new Event("online"));
  });
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

test("on touch the mailbox's name opens its menu, whose Select opens the selection", async () => {
  stubWidthQueries({ wide: true, coarse: true });
  const start = vi.fn<() => void>();
  const lent = lendSelection({ start, close: () => false });
  const unregister = pickable();
  try {
    await header();
    const trigger = await screen.findByRole("button", { name: NAME });
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    expect(screen.getByRole("heading", { level: 1 }).contains(trigger)).toBe(true);
    expect(await menuItems(NAME)).toEqual(["Select", "Mark all as read"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Select" }));
    await vi.waitFor(() => {
      expect(start).toHaveBeenCalledOnce();
    });
  } finally {
    unregister();
    lent();
  }
});

test("under a mouse the menu holds Mark all as read while the mailbox has unread mail", async () => {
  stubWidthQueries({ wide: true });
  const cache = await header();
  expect(await menuItems(NAME)).toEqual(["Mark all as read"]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Mark all as read" }));
  await vi.waitFor(() => {
    expect(cache.jobs).toEqual([{ kind: "markRead", mailboxId: INBOX_ID }]);
  });
});

test("the name stays plain where its menu would hold nothing", async () => {
  stubWidthQueries({ wide: true, coarse: true });
  const unregister = pickable();
  await header(boxOf("sent"));
  unregister();
  cleanup();
  stubWidthQueries({ wide: true });
  await header(boxOf("sent"));
  const name = screen.getByRole("heading", { level: 1 });
  expect(name.textContent).toBe("Sent");
  expect(within(name).queryByRole("button")).toBeNull();
});

test("Junk holds Empty Junk under a rule, which asks the dialog with Cancel first", async () => {
  stubWidthQueries({ wide: true });
  const cache = await header(boxOf("junk"));
  expect(await menuItems("Junk")).toEqual(["Mark all as read", "Empty Junk"]);
  expect(screen.getByRole("separator")).toBeTruthy();
  expect(screen.getByRole("menuitem", { name: "Empty Junk" }).dataset["danger"]).toBe("true");
  fireEvent.click(screen.getByRole("menuitem", { name: "Empty Junk" }));
  const dialog = await screen.findByRole("dialog", { name: "Empty Junk?" });
  expect(dialog.textContent).toContain("9 messages will be deleted forever. This can’t be undone.");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Cancel" }));
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await vi.waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  expect(cache.jobs).toEqual([]);
  fireEvent.click(await screen.findByRole("button", { name: "Junk" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Empty Junk" }));
  const again = await screen.findByRole("dialog", { name: "Empty Junk?" });
  fireEvent.click(within(again).getByRole("button", { name: "Empty Junk" }));
  expect(cache.jobs).toEqual([{ kind: "empty", mailboxId: boxOf("junk").id }]);
});

test("asking to empty keeps the page's keys until the dialog holds the focus", async () => {
  const run = vi.fn<() => void>();
  const unregister = registerCommand({
    id: "test.z",
    label: "z",
    group: "act",
    keys: [{ key: "z" }],
    run,
  });
  const uninstall = installCommandListener();
  render(<ConfirmEmpty locale="en" />);
  act(() => {
    askEmpty({ mailbox: boxOf("junk"), confirm: nothing });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", cancelable: true }));
  });
  expect(run).not.toHaveBeenCalled();
  const dialog = await screen.findByRole("dialog", { name: "Empty Junk?" });
  const cancel = within(dialog).getByRole("button", { name: "Cancel" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(cancel);
  });
  fireEvent.click(cancel);
  await vi.waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", cancelable: true }));
  expect(run).toHaveBeenCalledOnce();
  unregister();
  uninstall();
});

test("offline a job runs nothing and the header says so until the connection is back", async () => {
  stubWidthQueries({ wide: true });
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  const cache = await header(boxOf("trash"));
  fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Empty Trash" }));
  const sentence = "Offline: this will work again when the connection is back.";
  await vi.waitFor(() => {
    expect(screen.getByRole("banner").textContent).toContain(sentence);
  });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.queryByRole("heading")).toBeNull();
  expect(screen.getByRole("status").textContent).toBe(sentence);
  expect(cache.jobs).toEqual([]);
  act(() => {
    window.dispatchEvent(new Event("online"));
  });
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Trash");
});

test("a job the worker never takes ends as failed with nothing done", async () => {
  stubWidthQueries({ wide: true });
  const cache = await header();
  vi.spyOn(cache, "job").mockRejectedValue(new Error("gone"));
  const logged = vi.spyOn(console, "error").mockImplementation(nothing);
  expect(await menuItems(NAME)).toEqual(["Mark all as read"]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Mark all as read" }));
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent).toBe(
      "Couldn’t mark Inbox as read. Nothing was marked.",
    );
  });
  expect(logged).toHaveBeenCalledWith("mail: a folder job did not start", "gone");
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(NAME);
});

test("a running job takes the header with its count, Cancel and the hairline", async () => {
  stubWidthQueries({ wide: true });
  const cache = await header();
  const cancel = vi.spyOn(cache, "cancelJob");
  act(() => {
    heardJob(FASTMAIL.id, progress("running"));
  });
  const banner = screen.getByRole("banner");
  expect(banner.textContent).toContain("Emptying Trash…");
  expect(banner.textContent).toContain("500 of 1,204");
  expect(screen.queryByRole("heading")).toBeNull();
  expect(banner.querySelector<HTMLElement>("[aria-hidden='true'][style]")?.style.inlineSize).toBe(
    "42%",
  );
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent).toBe("Emptying Trash…");
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(cancel).toHaveBeenCalledWith(FASTMAIL.id);
  act(() => {
    heardJob(FASTMAIL.id, progress("canceled"));
  });
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(NAME);
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent).toBe("Stopped after 500 of 1,204.");
  });
});

test("Cancel holding the focus hands it to the mailbox's menu once the job ends", async () => {
  stubWidthQueries({ wide: true });
  await header();
  act(() => {
    heardJob(FASTMAIL.id, progress("running"));
  });
  act(() => {
    screen.getByRole("button", { name: "Cancel" }).focus();
  });
  act(() => {
    heardJob(FASTMAIL.id, progress("done", 1204));
  });
  expect(document.activeElement).toBe(await screen.findByRole("button", { name: NAME }));
});

test("the name's button holding the focus hands it to Cancel once a job starts", async () => {
  stubWidthQueries({ wide: true });
  await header();
  const trigger = await screen.findByRole("button", { name: NAME });
  act(() => {
    trigger.focus();
  });
  act(() => {
    heardJob(FASTMAIL.id, progress("running"));
  });
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
});

// The fixtures' tree with every message read, which leaves the inbox's
// menu nothing to hold.
const READ: Mailbox[] = WRITABLE.map((box) => ({ ...box, unreadEmails: 0 }));

test("the name's button leaving with the last unread hands the focus to the list", async () => {
  stubWidthQueries({ wide: true });
  const cache = fixtureCache({});
  await act(async () => {
    await prefetchMenus();
  });
  const list = <div tabIndex={-1} aria-label="Threads" />;
  const { rerender } = render(
    <>
      {pane(cache, INBOX, WRITABLE)}
      {list}
    </>,
  );
  const trigger = await screen.findByRole("button", { name: NAME });
  act(() => {
    trigger.focus();
  });
  rerender(
    <>
      {pane(
        cache,
        READ.find((box) => box.id === INBOX_ID),
        READ,
      )}
      {list}
    </>,
  );
  const name = screen.getByRole("heading", { level: 1 });
  expect(name.textContent).toBe(NAME);
  expect(within(name).queryByRole("button")).toBeNull();
  expect(document.activeElement).toBe(screen.getByLabelText("Threads"));
});

test("a job no word of reaches for a while leaves the header", async () => {
  stubWidthQueries({ wide: true });
  await header();
  vi.useFakeTimers();
  act(() => {
    heardJob(FASTMAIL.id, progress("running"));
  });
  expect(screen.queryByRole("heading")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(JOB_LAPSE_MS);
  });
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(NAME);
});

test("the header's search button names its key and runs the search the shell registers", async () => {
  stubWidthQueries({ wide: true });
  const run = vi.fn<() => void>();
  const unregister = registerCommand({
    id: "search.open",
    label: "Search mail",
    group: "app",
    keys: [{ key: "/" }],
    run,
  });
  try {
    await header();
    const button = screen.getByRole("button", { name: "Search mail" });
    expect(button.getAttribute("aria-keyshortcuts")).toBe("/");
    run.mockImplementation(() => {
      expect(document.activeElement).toBe(button);
    });
    fireEvent.click(button);
    expect(run).toHaveBeenCalledOnce();
  } finally {
    unregister();
  }
});

// The jobs as the palette lists them: each command's id and label.
function ids(): string[] {
  return commandsSnapshot()
    .filter((command) => command.id.startsWith("mailbox."))
    .map((command) => `${command.id} ${command.label}`);
}

test("the jobs the mailbox offers are in the palette, and none while a job runs", async () => {
  stubWidthQueries({ wide: true });
  await header(boxOf("trash"));
  await vi.waitFor(() => {
    expect(ids()).toEqual(["mailbox.empty Empty Trash"]);
  });
  act(() => {
    heardJob(FASTMAIL.id, progress("running"));
  });
  await vi.waitFor(() => {
    expect(ids()).toEqual([]);
  });
});
