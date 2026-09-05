// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { UserRow } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { useUserFlow } from "./use-user-flow";

const ONE_TIME = "k7fq-2mzp-x4rt";
const JONAS: UserRow = {
  id: "u-2",
  name: "Jonas",
  login: "jonas@example.com",
  role: "member",
  lastActiveAt: null,
};

// The flow's faces as buttons, so a test can walk them like a user.
function Harness() {
  const flow = useUserFlow("en");
  return (
    <>
      <output>{flow.issued?.secret ?? "none"}</output>
      <button
        type="button"
        onClick={() => {
          flow.start({ kind: "reset", user: JONAS });
        }}
      >
        start
      </button>
      <button type="button" onClick={flow.confirmReset}>
        confirm
      </button>
      <button type="button" onClick={flow.close}>
        close
      </button>
      <button type="button" onClick={flow.settle}>
        settle
      </button>
    </>
  );
}

// The hook reaches the router through the sign-out path, so the test mounts one.
async function renderHarness(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  const rootRoute = createRootRoute({ component: Harness });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByText("none");
  return queryClient;
}

function press(name: string): void {
  fireEvent.click(screen.getByRole("button", { name }));
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function answerWithSecret(): void {
  vi.stubGlobal(
    "fetch",
    vi
      .fn<typeof fetch>()
      .mockResolvedValue(
        new Response(JSON.stringify({ oneTimePassword: ONE_TIME, expiresAt: 1_778_836_800_000 })),
      ),
  );
}

test("a settled dialog leaves no one-time password behind, not even in the mutation cache", async () => {
  answerWithSecret();
  const queryClient = await renderHarness();
  press("start");
  press("confirm");
  await screen.findByText(ONE_TIME);
  press("close");
  press("settle");
  expect(await screen.findByText("none")).toBeDefined();
  await vi.waitFor(() => {
    expect(queryClient.getMutationCache().getAll()).toEqual([]);
  });
});

test("a fresh start inside the closing fade shows no previous answer", async () => {
  answerWithSecret();
  await renderHarness();
  press("start");
  press("confirm");
  await screen.findByText(ONE_TIME);
  press("close");
  press("start");
  expect(await screen.findByText("none")).toBeDefined();
});
