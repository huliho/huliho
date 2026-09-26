// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { dispatchKey } from "../commands/registry";
import { AccountSwitcher, prefetchAccountMenu } from "./account-switcher";
import {
  ACCOUNTS,
  EXPIRED,
  FASTMAIL,
  MAILBOXES,
  MAILBOXES_EMPTY_INBOX,
  STOPPED,
  fixtureCache,
} from "./fixtures";

const signOut = vi.hoisted(() => vi.fn<() => void>());
vi.mock("../auth/use-sign-out", () => ({ useSignOut: () => signOut }));

const navigated = vi.fn<() => void>();
const MARKED = [...ACCOUNTS, EXPIRED, STOPPED];
// Long enough for the test runner to transform the menu's chunk once.
const CHUNK_TIMEOUT_MS = 15_000;

// Each account's tree: the fixtures' for the first and the last, an
// inbox with nothing unread for the second, no tree yet for the third.
const cache: MailCache = {
  ...fixtureCache({}),
  mailboxes: (accountId) => {
    if (accountId === EXPIRED.id) {
      return Promise.resolve([]);
    }
    return Promise.resolve(accountId === "acc-2" ? MAILBOXES_EMPTY_INBOX : MAILBOXES);
  },
};

function Screen() {
  return (
    <>
      <AccountSwitcher
        locale="en"
        cache={cache}
        accounts={MARKED}
        account={FASTMAIL}
        variant="full"
        onNavigate={navigated}
      />
      <Outlet />
    </>
  );
}

// The switcher navigates and reads the trees, so it needs the routes it
// names and a query client.
function renderSwitcher() {
  const rootRoute = createRootRoute({ component: Screen });
  const mailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/mail/$accountId",
    component: () => <p>mail</p>,
  });
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    component: () => <p>settings</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([mailRoute, settingsRoute]),
    history: createMemoryHistory({ initialEntries: ["/mail/acc-1"] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

async function openMenu(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", { name: /Fastmail/ });
  fireEvent.click(trigger);
  return screen.findByRole("menu");
}

beforeEach(() => {
  vi.stubGlobal("scrollTo", vi.fn<typeof scrollTo>());
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  signOut.mockReset();
  navigated.mockReset();
});

test(
  "the trigger stands in until the menu's code is in; a press on it opens the menu once it lands",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    renderSwitcher();
    const trigger = await screen.findByRole("button", { name: /Fastmail/ });
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    // The press and the landing in one act, so the render after the
    // chunk lands is flushed; the first import transforms the menu's code.
    await act(async () => {
      fireEvent.click(trigger);
      await prefetchAccountMenu();
    });
    expect(screen.getByRole("menu")).toBeDefined();
    expect(screen.getByRole("button", { name: /Fastmail/ }).getAttribute("aria-expanded")).toBe(
      "true",
    );
  },
);

test("the menu lists every account with the open one checked, then Settings and Sign out", async () => {
  renderSwitcher();
  const menu = await openMenu();
  // The counts land once the trees are read: the number drawn, the words read out.
  await screen.findAllByText("23 unread");
  const accounts = screen.getAllByRole("menuitemradio");
  expect(accounts.map((row) => row.textContent)).toEqual([
    "FFastmailsanne@fastmail.com2323 unread",
    "GGmails.bakker@gmail.com",
    "KSKastanje StudioExpireds.bakker@kastanje.studio",
    "NNoordwindStoppedsanne@noordwind.nl2323 unread",
  ]);
  expect(accounts[0]?.getAttribute("aria-checked")).toBe("true");
  expect(accounts[1]?.getAttribute("aria-checked")).toBe("false");
  expect(screen.getByRole("menuitem", { name: "Settings" }).getAttribute("href")).toBe("/settings");
  expect(menu.textContent).toContain("Sign out");
});

test("each row carries its inbox's unread count in words and its mark word in the cause's tone", async () => {
  renderSwitcher();
  await openMenu();
  const [fastmail, gmail, expired, stopped] = screen.getAllByRole("menuitemradio");
  if (
    fastmail === undefined ||
    gmail === undefined ||
    expired === undefined ||
    stopped === undefined
  ) {
    throw new Error("the menu lists fewer accounts than the session holds");
  }
  expect(await within(fastmail).findByText("23 unread")).toBeDefined();
  expect(within(gmail).queryByText(/unread/)).toBeNull();
  expect(within(expired).queryByText(/unread/)).toBeNull();
  const expiredMark = within(expired).getByText("Expired");
  expect(expiredMark.className).toContain("markDanger");
  const stoppedMark = within(stopped).getByText("Stopped");
  expect(stoppedMark.className).toContain("markWarn");
  expect(within(stopped).getByText("23 unread")).toBeDefined();
});

test("choosing another account opens it, closes the menu and tells the caller", async () => {
  const router = renderSwitcher();
  await openMenu();
  fireEvent.click(screen.getByRole("menuitemradio", { name: /Gmail/ }));
  await vi.waitFor(() => {
    expect(router.state.location.pathname).toBe("/mail/acc-2");
    expect(screen.queryByRole("menu")).toBeNull();
  });
  expect(navigated).toHaveBeenCalledOnce();
});

test("Sign out runs the sign-out", async () => {
  renderSwitcher();
  await openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
  expect(signOut).toHaveBeenCalledOnce();
});

test("the switch command opens the menu from anywhere and Escape closes it", async () => {
  renderSwitcher();
  await screen.findByRole("button", { name: /Fastmail/ });
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key: "L", ctrlKey: true, shiftKey: true }));
  });
  const menu = await screen.findByRole("menu");
  expect(screen.getByRole("button", { name: /Fastmail/ }).getAttribute("aria-expanded")).toBe(
    "true",
  );
  fireEvent.keyDown(menu, { key: "Escape" });
  await vi.waitFor(() => {
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
