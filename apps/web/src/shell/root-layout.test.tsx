// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { CACHE_CHANNEL } from "../cache/messages";
import { RootLayout } from "./root-layout";

const REFUSED = { kind: "refused" };
const STOPPED = { kind: "account", accountId: "a1", stoppedCause: "connection" };
const SENTENCE = "Couldn’t mark as read. Your mail is safe.";

// The root layout in a router of its own, its listeners installed.
async function mounted(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  const rootRoute = createRootRoute({ component: RootLayout });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await act(() => router.load());
  return queryClient;
}

function seenAs(state: DocumentVisibilityState): void {
  vi.spyOn(document, "visibilityState", "get").mockReturnValue(state);
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("a change the server refused is said in the tab in view that holds the session and in no other", async () => {
  const queryClient = await mounted();
  const refetched = vi.spyOn(queryClient, "invalidateQueries");
  const worker = new BroadcastChannel(CACHE_CHANNEL);
  const post = worker.postMessage.bind(worker);
  // The channel keeps its order, so a refusal was heard once the
  // message behind it is.
  const refusedAndHeard = async (times: number): Promise<void> => {
    post(REFUSED);
    post(STOPPED);
    await vi.waitFor(() => {
      expect(refetched).toHaveBeenCalledTimes(times);
    });
  };
  // In view without a session, as the link page and the sign-in screen are.
  seenAs("visible");
  await refusedAndHeard(1);
  expect(screen.queryByText(SENTENCE)).toBeNull();
  queryClient.setQueryData(queryKeys.session, { privacyStrict: false });
  seenAs("hidden");
  await refusedAndHeard(2);
  expect(screen.queryByText(SENTENCE)).toBeNull();
  seenAs("visible");
  post(REFUSED);
  await screen.findByText(SENTENCE);
  expect(screen.getAllByText(SENTENCE)).toHaveLength(1);
  worker.close();
});
