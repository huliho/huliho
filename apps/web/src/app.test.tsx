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
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";

import { App } from "./app";
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
});

test("switching the locale translates the page without a reload", async () => {
  await renderApp();
  expect(screen.getByText("Your mail, wherever it lives.")).toBeDefined();

  fireEvent.change(screen.getByLabelText("Language"), { target: { value: "nl" } });

  expect(await screen.findByText("Je mail, waar die ook staat.")).toBeDefined();
  expect(document.documentElement.lang).toBe("nl");
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("nl");
});

test("dates and numbers format per locale through Intl", async () => {
  await renderApp();
  expect(screen.getByText(/24,817 messages/)).toBeDefined();

  fireEvent.change(screen.getByLabelText("Language"), { target: { value: "nl" } });

  expect(await screen.findByText(/24\.817 berichten/)).toBeDefined();
  expect(screen.getByText(/Vandaag is het/)).toBeDefined();
});
