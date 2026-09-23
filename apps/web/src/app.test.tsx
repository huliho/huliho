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
import { afterEach, beforeEach, expect, test } from "vitest";

import { App } from "./app";
import { switchLocale } from "./i18n/locale";
import { setLocale } from "./paraglide/runtime.js";

beforeEach(async () => {
  localStorage.clear();
  await setLocale("en", { reload: false });
});

afterEach(cleanup);

// The shell reads router and query context, so the test mounts both.
async function renderApp(): Promise<void> {
  const rootRoute = createRootRoute({ component: App });
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory(),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByRole("main");
}

test("mounts the application shell", async () => {
  await renderApp();
  expect(screen.getByRole("heading", { level: 1, name: "Huliho" })).toBeDefined();
  expect(screen.getByText("Your mail, wherever it lives.")).toBeDefined();
  expect(screen.getByText(/24,817 messages/)).toBeDefined();
});

test("a locale switch translates the mounted shell and its Intl formatting", async () => {
  await renderApp();
  act(() => {
    switchLocale("nl");
  });
  expect(await screen.findByText("Je mail, waar die ook staat.")).toBeDefined();
  expect(screen.getByText(/24\.817 berichten/)).toBeDefined();
  expect(screen.getByText(/Vandaag is het/)).toBeDefined();
  expect(document.documentElement.lang).toBe("nl");
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("nl");
});
