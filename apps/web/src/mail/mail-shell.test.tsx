// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { Mailbox } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "../cache/coordinator";
import { ACCOUNTS, MAILBOXES } from "./fixtures";
import { InboxRedirect } from "./inbox-redirect";
import { LIST_WIDTH_DEFAULT_PX, PANE_MIN_WIDTH_PX } from "./list-width";
import { MailShell } from "./mail-shell";
import { MailboxPane } from "./mailbox-pane";

const mailboxes = vi.hoisted(() => vi.fn<(accountId: string) => Promise<Mailbox[]>>());
const attach = vi.hoisted(() => vi.fn<(lease: Lease) => () => void>(() => () => undefined));
vi.mock("../cache/client", () => ({
  mailCache: {
    mailboxes,
    window: vi.fn<() => Promise<never>>(),
    thread: vi.fn<() => Promise<never>>(),
    reveal: vi.fn<() => Promise<never>>(),
  },
  attachCache: attach,
  clearCache: vi.fn<() => Promise<void>>(() => Promise.resolve()),
}));

const PROBE_INTERVAL_MINUTES = 15;
// A frame and a side panel, in CSS pixels, that leave less room than the stored width asks.
const FRAME_PX = 1200;
const NARROWER_FRAME_PX = 1000;
const SIDE_PX = 240;
const STORED_LIST_WIDTH_PX = 1360;
// A frame that leaves the design's default less room than it asks.
const TIGHT_FRAME_PX = 900;

// The shell at the desktop width, with the accounts the guard would have fetched.
function renderShell(path: string) {
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
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      signedInRoute.addChildren([homeRoute, mailRoute.addChildren([indexRoute, mailboxRoute])]),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.accounts, {
    accounts: ACCOUNTS,
    probeIntervalMinutes: PROBE_INTERVAL_MINUTES,
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

beforeEach(() => {
  localStorage.clear();
  mailboxes.mockReset();
  mailboxes.mockResolvedValue(MAILBOXES);
  attach.mockClear();
  vi.stubGlobal("scrollTo", vi.fn<typeof scrollTo>());
  // Every width query matches: the desktop layout.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: true,
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
  expect(screen.getAllByRole("treeitem")).toHaveLength(MAILBOXES.length);
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
