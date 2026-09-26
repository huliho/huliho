// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { AccountSwitcher, prefetchAccountMenu } from "./account-switcher";
import { ACCOUNTS, FASTMAIL, fixtureCache } from "./fixtures";

vi.mock("../auth/use-sign-out", () => ({ useSignOut: () => vi.fn<() => void>() }));

// Long enough for the test runner to transform the menu's chunk once.
const CHUNK_TIMEOUT_MS = 15_000;

// The menu's chunk lands once per module graph, so the file holds the
// one test that needs the stand-in still in place.
function renderSwitcher(): void {
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => (
        <AccountSwitcher
          locale="en"
          cache={fixtureCache({})}
          accounts={ACCOUNTS}
          account={FASTMAIL}
          variant="full"
        />
      ),
    }),
    history: createMemoryHistory({ initialEntries: ["/mail/acc-1"] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

test(
  "a trigger that holds the focus when the menu's code lands hands it to the landed trigger",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    renderSwitcher();
    const standIn = await screen.findByRole("button", { name: /Fastmail/ });
    expect(standIn.hasAttribute("id")).toBe(false);
    act(() => {
      standIn.focus();
    });
    expect(document.activeElement).toBe(standIn);
    await act(async () => {
      await prefetchAccountMenu();
    });
    const landed = screen.getByRole("button", { name: /Fastmail/ });
    expect(landed).not.toBe(standIn);
    expect(landed.hasAttribute("id")).toBe(true);
    expect(document.activeElement).toBe(landed);
    expect(screen.queryByRole("menu")).toBeNull();
  },
);
