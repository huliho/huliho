// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { ListPage, Mailbox, ThreadDetail } from "@huliho/core";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "../cache/coordinator";
import { INBOX_PAGE, MAILBOXES, THREAD_ID } from "./fixtures";
import { LIST_DEFAULT_ROWS, LIST_MIN_ROWS, PANE_MIN_HEIGHT_PX } from "./list-height";
import { LIST_WIDTH_DEFAULT_PX, PANE_MIN_WIDTH_PX } from "./list-width";
import { FRAME_HEIGHT_PX, command, layout, mockShellBox, renderShell, rows } from "./shell-rig";

const mailboxes = vi.hoisted(() => vi.fn<(accountId: string) => Promise<Mailbox[]>>());
const attach = vi.hoisted(() => vi.fn<(lease: Lease) => () => void>(() => () => undefined));
const polled = vi.hoisted(() => vi.fn<() => void>());
vi.mock("../cache/client", async () => {
  const fixtures = await import("./fixtures");
  return {
    mailCache: {
      mailboxes,
      window: vi.fn<() => Promise<ListPage>>(() => Promise.resolve(fixtures.INBOX_PAGE)),
      thread: vi.fn<() => Promise<ThreadDetail>>(() => Promise.resolve(fixtures.THREAD)),
      reveal: vi.fn<() => Promise<never>>(),
    },
    attachCache: attach,
    clearCache: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    pollCache: polled,
  };
});

// A frame and a side panel, in CSS pixels, that leave less room than the stored width asks.
const FRAME_PX = 1200;
const NARROWER_FRAME_PX = 1000;
const SIDE_PX = 240;
const STORED_LIST_WIDTH_PX = 1360;
// A frame that leaves the design's default less room than it asks.
const TIGHT_FRAME_PX = 900;
// A frame too short for the list's least height and the pane's together.
const SHORT_FRAME_HEIGHT_PX = 400;
const THREAD_PATH = `/mail/acc-1/mb-inbox/${THREAD_ID}`;
const SUBJECT = "Offerte badkamerrenovatie, herziene versie";

mockShellBox();

beforeEach(() => {
  mailboxes.mockReset();
  mailboxes.mockResolvedValue(MAILBOXES);
  attach.mockClear();
});

test("the shell names the mailbox, draws the tree and leases the worker the watched mailbox", async () => {
  renderShell("/mail/acc-1/mb-inbox");
  expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeDefined();
  expect(screen.getByText("23 unread")).toBeDefined();
  expect(screen.getAllByRole("treeitem")).toHaveLength(MAILBOXES.length);
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  expect(grid.getAttribute("aria-rowcount")).toBe(String(INBOX_PAGE.rows.length));
  expect(
    screen.getByRole("treeitem", { name: "Inbox, 23 unread" }).getAttribute("aria-current"),
  ).toBe("page");
  expect(screen.getByRole("complementary", { name: "Conversation" }).textContent).toBe(
    "Select a conversation.",
  );
  expect(attach).toHaveBeenCalledWith(
    expect.objectContaining({
      accounts: ["acc-1", "acc-2"],
      watching: { accountId: "acc-1", mailboxId: "mb-inbox" },
    }),
  );
  expect(localStorage.getItem("huliho-last-account")).toBe("acc-1");
});

test("a mailbox opened after another starts at its first row", async () => {
  renderShell("/mail/acc-1/mb-inbox");
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  const first = await vi.waitFor(() => {
    const row = grid.querySelector('[aria-rowindex="1"]');
    if (!(row instanceof HTMLElement)) {
      throw new Error("row 1 is not in yet");
    }
    return row;
  });
  act(() => {
    first.focus();
  });
  fireEvent.keyDown(first, { key: "End" });
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe(
      String(INBOX_PAGE.rows.length),
    );
  });
  fireEvent.click(screen.getByRole("treeitem", { name: "Sent" }));
  expect(await screen.findByRole("heading", { level: 1, name: "Sent" })).toBeDefined();
  const next = await screen.findByRole("grid", { name: "Conversations" });
  await vi.waitFor(() => {
    const stop = next.querySelector('[role="row"][tabindex="0"]');
    expect(stop?.getAttribute("aria-rowindex")).toBe("1");
  });
});

test("g then a letter jumps to that mailbox with the focus on its first row", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  await screen.findByRole("grid", { name: "Conversations" });
  command("g");
  command("d");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-drafts");
  });
  expect(await screen.findByRole("heading", { level: 1, name: "Drafts" })).toBeDefined();
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe("1");
  });
  // A folder jumps by the letter the tree shows for it.
  command("g");
  command("f");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-facturen");
  });
});

test("a back onto a mailbox a jump opened leaves the focus where it is", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  await screen.findByRole("grid", { name: "Conversations" });
  command("g");
  command("d");
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe("1");
  });
  expect(router.state.location.pathname).toBe("/mail/acc-1/mb-drafts");
  fireEvent.click(screen.getByRole("treeitem", { name: "Sent" }));
  await screen.findByRole("heading", { level: 1, name: "Sent" });
  const card = screen.getByRole("button", { name: (name) => name.includes("sanne@fastmail.com") });
  act(() => {
    card.focus();
  });
  act(() => {
    router.history.back();
  });
  await screen.findByRole("heading", { level: 1, name: "Drafts" });
  await vi.waitFor(() => {
    const stop = screen.getByRole("grid").querySelector('[role="row"][tabindex="0"]');
    expect(stop?.getAttribute("aria-rowindex")).toBe("1");
  });
  expect(document.activeElement).toBe(card);
});

test("an account opened without a mailbox goes to its inbox", async () => {
  const router = renderShell("/mail/acc-1");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
});

test("a mailbox the tree lacks goes to the inbox", async () => {
  const router = renderShell("/mail/acc-1/mb-nowhere");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
});

test("an empty mailbox says so; an account without mailboxes says that", async () => {
  renderShell("/mail/acc-1/mb-verbouwing");
  expect(await screen.findByText("Verbouwing is empty.")).toBeDefined();
  expect(screen.getByRole("link", { name: "Open Inbox" }).getAttribute("href")).toBe(
    "/mail/acc-1/mb-inbox",
  );
  mailboxes.mockResolvedValue([]);
  renderShell("/mail/acc-2");
  expect(await screen.findByText("This account has no mailboxes to show yet.")).toBeDefined();
});

test("a tree that fails to load says so and Try again fetches it again", async () => {
  mailboxes.mockRejectedValueOnce(new JmapError("unavailable"));
  renderShell("/mail/acc-1/mb-inbox");
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Couldn’t load your mail.");
  expect(screen.queryByRole("tree")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeDefined();
  // The switcher reads the other account's tree beside; this one was asked twice.
  expect(mailboxes.mock.calls.filter(([accountId]) => accountId === "acc-1")).toHaveLength(2);
});

test("an account the session does not hold sends the visit to the root", async () => {
  const router = renderShell("/mail/acc-9/mb-inbox");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/");
  });
});

test("a width stored on a wider window is clamped so the reading pane keeps its minimum", async () => {
  const frameWidth = vi
    .spyOn(HTMLElement.prototype, "clientWidth", "get")
    .mockReturnValue(FRAME_PX);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(SIDE_PX);
  localStorage.setItem("huliho-list-width", String(STORED_LIST_WIDTH_PX));
  renderShell("/mail/acc-1/mb-inbox");
  expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeDefined();
  const main = screen.getByRole("main");
  const seam = screen.getByRole("separator", { name: "Resize the list" });
  const room = FRAME_PX - SIDE_PX - PANE_MIN_WIDTH_PX;
  expect(main.style.inlineSize).toBe(`${String(room)}px`);
  expect(seam.getAttribute("aria-valuenow")).toBe(String(room));
  expect(main.id).not.toBe("");
  expect(seam.getAttribute("aria-controls")).toBe(main.id);
  expect(seam.getAttribute("aria-valuemin")).toBe(String(PANE_MIN_WIDTH_PX));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(room));
  frameWidth.mockReturnValue(NARROWER_FRAME_PX);
  act(() => {
    window.dispatchEvent(new Event("resize"));
  });
  const narrower = NARROWER_FRAME_PX - SIDE_PX - PANE_MIN_WIDTH_PX;
  expect(main.style.inlineSize).toBe(`${String(narrower)}px`);
  expect(seam.getAttribute("aria-valuenow")).toBe(String(narrower));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(narrower));
});

test("a thread in the address opens beside the list with its row drawn selected; Escape closes it", async () => {
  const router = renderShell(THREAD_PATH);
  const pane = await screen.findByRole("complementary", { name: "Conversation" });
  const title = await within(pane).findByRole("heading", { level: 2, name: SUBJECT });
  expect(document.activeElement).toBe(title);
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  await vi.waitFor(() => {
    expect(grid.querySelector('[aria-rowindex="3"]')?.getAttribute("aria-selected")).toBe("true");
  });
  command("Escape");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  await vi.waitFor(() => {
    expect(pane.textContent).toBe("Select a conversation.");
  });
  expect(document.activeElement?.getAttribute("role")).toBe("row");
  expect(grid.querySelector('[aria-selected="true"]')).toBeNull();
});

test("a thread opened from the list closes by going back over its entry; a second open replaces it", async () => {
  const router = renderShell("/mail/acc-1/mb-inbox");
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  const second = INBOX_PAGE.rows[1]?.threadId ?? "";
  const third = INBOX_PAGE.rows[2]?.threadId ?? "";
  // Each open is done once its row is drawn selected, as a hand would see it.
  const opened = async (index: number, threadId: string): Promise<void> => {
    fireEvent.click(grid.querySelector(`[aria-rowindex="${String(index)}"]`) ?? grid);
    await vi.waitFor(() => {
      expect(router.state.location.pathname).toBe(`/mail/acc-1/mb-inbox/${threadId}`);
      expect(grid.querySelector('[aria-selected="true"]')?.getAttribute("aria-rowindex")).toBe(
        String(index),
      );
    });
  };
  await opened(2, second);
  await opened(3, third);
  command("Escape");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  act(() => {
    router.history.forward();
  });
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe(`/mail/acc-1/mb-inbox/${third}`);
  });
});

test("a thread reached by its address stays unmarked through a second open, so closing leaves the mailbox in its place", async () => {
  const router = renderShell(THREAD_PATH);
  const grid = await screen.findByRole("grid", { name: "Conversations" });
  const second = INBOX_PAGE.rows[1]?.threadId ?? "";
  fireEvent.click(grid.querySelector('[aria-rowindex="2"]') ?? grid);
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe(`/mail/acc-1/mb-inbox/${second}`);
    expect(grid.querySelector('[aria-selected="true"]')?.getAttribute("aria-rowindex")).toBe("2");
  });
  expect(router.history.length).toBe(1);
  command("Escape");
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  expect(router.history.length).toBe(1);
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe("2");
  });
});

test("below the list the seam turns and sizes the list in rows under its header", async () => {
  renderShell(THREAD_PATH, { readingPane: "bottom" });
  await screen.findByRole("heading", { level: 2, name: SUBJECT });
  const main = screen.getByRole("main");
  const seam = screen.getByRole("separator", { name: "Resize the list" });
  expect(seam.getAttribute("aria-orientation")).toBe("horizontal");
  expect(seam.getAttribute("aria-valuenow")).toBe(String(rows(LIST_DEFAULT_ROWS)));
  expect(seam.getAttribute("aria-valuemin")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(FRAME_HEIGHT_PX - PANE_MIN_HEIGHT_PX));
  expect(main.style.blockSize).toBe(`${String(rows(LIST_DEFAULT_ROWS))}px`);
  expect(main.style.inlineSize).toBe("");
  fireEvent.keyDown(seam, { key: "ArrowDown" });
  expect(main.style.blockSize).toBe(`${String(rows(LIST_DEFAULT_ROWS + 1))}px`);
  expect(localStorage.getItem("huliho-list-height")).toBe(String(rows(LIST_DEFAULT_ROWS + 1)));
  fireEvent.keyDown(seam, { key: "Enter" });
  expect(main.style.blockSize).toBe(`${String(rows(LIST_DEFAULT_ROWS))}px`);
  expect(localStorage.getItem("huliho-list-height")).toBeNull();
});

test("a frame too short for the list and the pane keeps the list at its least", async () => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(SHORT_FRAME_HEIGHT_PX);
  renderShell(THREAD_PATH, { readingPane: "bottom" });
  await screen.findByRole("heading", { level: 2, name: SUBJECT });
  expect(SHORT_FRAME_HEIGHT_PX).toBeLessThan(rows(LIST_MIN_ROWS) + PANE_MIN_HEIGHT_PX);
  const seam = screen.getByRole("separator", { name: "Resize the list" });
  expect(seam.getAttribute("aria-valuemin")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(seam.getAttribute("aria-valuenow")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(screen.getByRole("main").style.blockSize).toBe(`${String(rows(LIST_MIN_ROWS))}px`);
});

test("with the pane off the thread is a screen over the list, which is out of reach until it closes", async () => {
  const router = renderShell(THREAD_PATH, { readingPane: "off" });
  const title = await screen.findByRole("heading", { level: 1, name: SUBJECT });
  expect(document.activeElement).toBe(title);
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(screen.queryByRole("separator")).toBeNull();
  expect(screen.getByRole("region", { name: "Conversation" })).toBeDefined();
  const main = document.querySelector("main");
  expect(main?.hasAttribute("inert")).toBe(true);
  expect(main?.style.inlineSize).toBe("");
  fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-1/mb-inbox");
  });
  await vi.waitFor(() => {
    expect(main?.hasAttribute("inert")).toBe(false);
  });
  expect(screen.queryByRole("region", { name: "Conversation" })).toBeNull();
});

test("a phone opens the thread as a screen whatever the preference says", async () => {
  layout.wide = false;
  renderShell(THREAD_PATH);
  await screen.findByRole("heading", { level: 1, name: SUBJECT });
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(screen.queryByRole("separator")).toBeNull();
  expect(document.querySelector("main")?.hasAttribute("inert")).toBe(true);
  expect(screen.getByRole("button", { name: "Back to Inbox" }).textContent).not.toContain("esc");
});

test("the design's default is clamped when the frame leaves less room", async () => {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(TIGHT_FRAME_PX);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(SIDE_PX);
  renderShell("/mail/acc-1/mb-inbox");
  expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeDefined();
  const room = TIGHT_FRAME_PX - SIDE_PX - PANE_MIN_WIDTH_PX;
  expect(room).toBeLessThan(LIST_WIDTH_DEFAULT_PX);
  expect(screen.getByRole("main").style.inlineSize).toBe(`${String(room)}px`);
  const seam = screen.getByRole("separator", { name: "Resize the list" });
  expect(seam.getAttribute("aria-valuenow")).toBe(String(room));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(room));
});
