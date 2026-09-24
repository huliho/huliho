// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { AccountSwitcher } from "./account-switcher";
import { ACCOUNTS, FASTMAIL } from "./fixtures";

const signOut = vi.hoisted(() => vi.fn<() => void>());
vi.mock("../auth/use-sign-out", () => ({ useSignOut: () => signOut }));

const navigated = vi.fn<() => void>();

function Screen() {
  return (
    <>
      <AccountSwitcher
        locale="en"
        accounts={ACCOUNTS}
        account={FASTMAIL}
        variant="full"
        onNavigate={navigated}
      />
      <Outlet />
    </>
  );
}

// The switcher navigates, so it needs the routes it names.
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
  render(<RouterProvider router={router} />);
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

test("the menu lists every account with the open one checked, then Settings and Sign out", async () => {
  renderSwitcher();
  const menu = await openMenu();
  const accounts = screen.getAllByRole("menuitemradio");
  expect(accounts.map((row) => row.textContent)).toEqual([
    "FFastmailsanne@fastmail.com",
    "GGmails.bakker@gmail.com",
  ]);
  expect(accounts[0]?.getAttribute("aria-checked")).toBe("true");
  expect(accounts[1]?.getAttribute("aria-checked")).toBe("false");
  expect(screen.getByRole("menuitem", { name: "Settings" }).getAttribute("href")).toBe("/settings");
  expect(menu.textContent).toContain("Sign out");
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
