// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { ListPage, Mailbox, ReadingPane, ThreadDetail } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "../cache/coordinator";
import { dispatchKey } from "../commands/registry";
import { ACCOUNTS, INBOX_PAGE, MAILBOXES, THREAD_ID } from "./fixtures";
import { InboxRedirect } from "./inbox-redirect";
import { LIST_DEFAULT_ROWS, LIST_MIN_ROWS, PANE_MIN_HEIGHT_PX } from "./list-height";
import { LIST_WIDTH_DEFAULT_PX, PANE_MIN_WIDTH_PX } from "./list-width";
import { MailShell } from "./mail-shell";
import { MailboxPane } from "./mailbox-pane";
import { ROW_HEIGHT_FALLBACK_PX, TOOLBAR_HEIGHT_FALLBACK_PX } from "./use-token-px";

const mailboxes = vi.hoisted(() => vi.fn<(accountId: string) => Promise<Mailbox[]>>());
const attach = vi.hoisted(() => vi.fn<(lease: Lease) => () => void>(() => () => undefined));
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
  };
});

const PROBE_INTERVAL_MINUTES = 15;
// A frame and a side panel, in CSS pixels, that leave less room than the stored width asks.
const FRAME_PX = 1200;
const NARROWER_FRAME_PX = 1000;
const SIDE_PX = 240;
const STORED_LIST_WIDTH_PX = 1360;
// A frame that leaves the design's default less room than it asks.
const TIGHT_FRAME_PX = 900;
// The list's box in the test, which jsdom gives no element.
const VIEW_HEIGHT_PX = 520;
// The frame's height, for the pane below the list.
const FRAME_HEIGHT_PX = 900;
// A frame too short for the list's least height and the pane's together.
const SHORT_FRAME_HEIGHT_PX = 400;
const THREAD_PATH = `/mail/acc-1/mb-inbox/${THREAD_ID}`;
const SUBJECT = "Offerte badkamerrenovatie, herziene versie";
// Whether the width queries match: every one at the desktop width, none on a phone.
let wide = true;

// The shell at the desktop width, with the accounts the guard would
// have fetched and the reading pane where the preference puts it.
function renderShell(path: string, readingPane: ReadingPane = "right") {
  const rootRoute = createRootRoute();
  const signedInRoute = createRoute({ getParentRoute: () => rootRoute, id: "signed-in" });
  const homeRoute = createRoute({
    getParentRoute: () => signedInRoute,
    path: "/",
    component: () => <p>home</p>,
  });
  const mailRoute = createRoute({
    getParentRoute: () => signedInRoute,
    path: "/mail/$accountId",
    component: MailShell,
  });
  const indexRoute = createRoute({
    getParentRoute: () => mailRoute,
    path: "/",
    component: InboxRedirect,
  });
  const mailboxRoute = createRoute({
    getParentRoute: () => mailRoute,
    path: "/$mailboxId",
    component: MailboxPane,
  });
  const threadRoute = createRoute({ getParentRoute: () => mailboxRoute, path: "/$threadId" });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      signedInRoute.addChildren([
        homeRoute,
        mailRoute.addChildren([indexRoute, mailboxRoute.addChildren([threadRoute])]),
      ]),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.accounts, {
    accounts: ACCOUNTS,
    probeIntervalMinutes: PROBE_INTERVAL_MINUTES,
  });
  queryClient.setQueryData(queryKeys.preferences, { readingPane });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

// The list's height at `count` rows under its header, at the fallback sizes jsdom leaves.
function rows(count: number): number {
  return TOOLBAR_HEIGHT_FALLBACK_PX + count * ROW_HEIGHT_FALLBACK_PX;
}

// jsdom has no ResizeObserver; the frame watches its side panel with
// one and the virtualizer its scroll box.
class StillObserver {
  observe(): void {
    return undefined;
  }

  unobserve(): void {
    return undefined;
  }

  disconnect(): void {
    return undefined;
  }
}

beforeEach(() => {
  localStorage.clear();
  wide = true;
  mailboxes.mockReset();
  mailboxes.mockResolvedValue(MAILBOXES);
  attach.mockClear();
  vi.stubGlobal("ResizeObserver", StillObserver);
  vi.stubGlobal("scrollTo", vi.fn<typeof scrollTo>());
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: vi.fn<() => void>(),
  });
  // Every box is the view's height, but the seam, which takes nothing of the flow.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.getAttribute("role") === "separator" ? 0 : VIEW_HEIGHT_PX;
  });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(FRAME_HEIGHT_PX);
  // Every width query matches: the desktop layout; none matches on a phone.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: wide,
    media: query,
    addEventListener() {
      return undefined;
    },
    removeEventListener() {
      return undefined;
    },
  }));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
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
  cleanup();
  mailboxes.mockResolvedValue([]);
  renderShell("/mail/acc-1");
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
  expect(mailboxes).toHaveBeenCalledTimes(2);
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
  renderShell(THREAD_PATH, "bottom");
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
  renderShell(THREAD_PATH, "bottom");
  await screen.findByRole("heading", { level: 2, name: SUBJECT });
  expect(SHORT_FRAME_HEIGHT_PX).toBeLessThan(rows(LIST_MIN_ROWS) + PANE_MIN_HEIGHT_PX);
  const seam = screen.getByRole("separator", { name: "Resize the list" });
  expect(seam.getAttribute("aria-valuemin")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(seam.getAttribute("aria-valuenow")).toBe(String(rows(LIST_MIN_ROWS)));
  expect(screen.getByRole("main").style.blockSize).toBe(`${String(rows(LIST_MIN_ROWS))}px`);
});

test("with the pane off the thread is a screen over the list, which is out of reach until it closes", async () => {
  const router = renderShell(THREAD_PATH, "off");
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
  wide = false;
  renderShell(THREAD_PATH);
  await screen.findByRole("heading", { level: 1, name: SUBJECT });
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(screen.queryByRole("separator")).toBeNull();
  expect(document.querySelector("main")?.hasAttribute("inert")).toBe(true);
  expect(screen.getByRole("button", { name: "Back to Inbox" }).textContent).not.toContain("Esc");
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
