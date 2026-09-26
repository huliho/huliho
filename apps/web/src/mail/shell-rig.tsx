// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, ReadingPane } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

import { dispatchKey } from "../commands/registry";
import { ACCOUNTS, PROBE_INTERVAL_MINUTES } from "./fixtures";
import { InboxRedirect } from "./inbox-redirect";
import { MailShell } from "./mail-shell";
import { MailboxPane } from "./mailbox-pane";
import { ROW_HEIGHT_FALLBACK_PX, TOOLBAR_HEIGHT_FALLBACK_PX } from "./use-token-px";

// The rig the shell tests render in: the mail routes on a memory
// history, the accounts the guard would have fetched and the boxes
// jsdom gives no element. The test file mocks the cache client itself.
// The frame's height, for the pane below the list.
export const FRAME_HEIGHT_PX = 900;
// The list's box in the test.
const VIEW_HEIGHT_PX = 520;

export interface ShellOptions {
  readingPane?: ReadingPane;
  // The accounts of the session; the fixtures' pair by default.
  accounts?: AccountRow[];
}

// Whether the width queries match: every one at the desktop width, none on a phone.
export const layout = { wide: true };

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

// The shell at the desktop width, with the accounts the guard would
// have fetched and the reading pane where the preference puts it.
export function renderShell(path: string, options: ShellOptions = {}) {
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
    accounts: options.accounts ?? ACCOUNTS,
    probeIntervalMinutes: PROBE_INTERVAL_MINUTES,
  });
  queryClient.setQueryData(queryKeys.preferences, { readingPane: options.readingPane ?? "right" });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

export function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

// The list's height at `count` rows under its header, at the fallback sizes jsdom leaves.
export function rows(count: number): number {
  return TOOLBAR_HEIGHT_FALLBACK_PX + count * ROW_HEIGHT_FALLBACK_PX;
}

// The boxes and the media queries of the shell for every test of the
// file: the desktop layout, a frame of the given height and a seam
// that takes nothing of the flow.
export function mockShellBox(): void {
  beforeEach(() => {
    localStorage.clear();
    layout.wide = true;
    vi.stubGlobal("ResizeObserver", StillObserver);
    vi.stubGlobal("scrollTo", vi.fn<typeof scrollTo>());
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn<() => void>(),
    });
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.getAttribute("role") === "separator" ? 0 : VIEW_HEIGHT_PX;
    });
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(FRAME_HEIGHT_PX);
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: layout.wide,
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
}
