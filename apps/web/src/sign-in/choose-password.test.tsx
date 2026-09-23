// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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
import type { Mock } from "vitest";

import { ToastProvider, Toasts } from "../design-system/toast";
import { setLocale } from "../paraglide/runtime.js";
import { useAppliedPreferences } from "../theme/use-applied-preferences";
import { ChoosePassword } from "./choose-password";

const NEXT = "a brand new passphrase";
const PREFERENCES_URL = "/api/preferences";
const PASSWORD_URL = "/api/password";
const FORBIDDEN = 403;
const NO_CONTENT = 204;

// The layout every guarded route shares: it stays mounted across the step.
function Layout() {
  useAppliedPreferences();
  return <Outlet />;
}

function Home() {
  return <main>home</main>;
}

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  return input instanceof URL ? input.href : input.url;
}

// Answers as the server does: the preferences are refused until the
// password change lands, then the words on record come through.
function answerLikeServer(): Mock<typeof fetch> {
  let forced = true;
  const fetchMock = vi.fn<typeof fetch>((input) => {
    const url = urlOf(input);
    if (url === PASSWORD_URL) {
      forced = false;
      return Promise.resolve(new Response(null, { status: NO_CONTENT }));
    }
    if (url === PREFERENCES_URL) {
      return Promise.resolve(
        forced
          ? new Response(JSON.stringify({ error: "password_change_required" }), {
              status: FORBIDDEN,
            })
          : new Response(JSON.stringify({ theme: "dark" })),
      );
    }
    return Promise.reject(new Error(`unexpected request to ${url}`));
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function preferenceCalls(fetchMock: Mock<typeof fetch>): number {
  return fetchMock.mock.calls.filter(([input]) => urlOf(input) === PREFERENCES_URL).length;
}

async function renderForcedStep(): Promise<QueryClient> {
  const rootRoute = createRootRoute({ component: Layout });
  const stepRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/choose-password",
    component: ChoosePassword,
  });
  const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: Home });
  const router = createRouter({
    routeTree: rootRoute.addChildren([stepRoute, homeRoute]),
    history: createMemoryHistory({ initialEntries: ["/choose-password"] }),
  });
  const queryClient = new QueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
        <Toasts />
      </ToastProvider>
    </QueryClientProvider>,
  );
  await screen.findByLabelText("New password");
  return queryClient;
}

function submitNewPassword(): void {
  fireEvent.change(screen.getByLabelText("New password"), { target: { value: NEXT } });
  fireEvent.change(screen.getByLabelText("Repeat it"), { target: { value: NEXT } });
  const form = screen.getByRole("button", { name: "Save and continue" }).closest("form");
  if (form === null) {
    throw new Error("the form is not rendered");
  }
  fireEvent.submit(form);
}

beforeEach(async () => {
  localStorage.clear();
  delete document.documentElement.dataset["theme"];
  // The router scrolls on navigation; jsdom has no scrollTo.
  vi.stubGlobal("scrollTo", vi.fn<typeof scrollTo>());
  await setLocale("en", { reload: false });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("the refused words leave the document alone; they apply once the change lands", async () => {
  const fetchMock = answerLikeServer();
  const queryClient = await renderForcedStep();
  await vi.waitFor(() => {
    expect(queryClient.getQueryState(queryKeys.preferences)?.status).toBe("error");
  });
  expect(document.documentElement.dataset["theme"]).toBeUndefined();

  submitNewPassword();
  await screen.findByText("home");
  expect(await screen.findByText("Password changed. Other devices were signed out.")).toBeDefined();
  await vi.waitFor(() => {
    expect(document.documentElement.dataset["theme"]).toBe("dark");
  });
  expect(preferenceCalls(fetchMock)).toBe(2);
});
