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
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";

const SESSION = {
  user: { id: "user-1", login: "mira@example.com", name: "Mira", role: "owner" },
  organization: { id: "org-1", name: "mira@example.com" },
  signInProviders: [],
  passwordChangeRequired: false,
};

// A hook that ends the session reaches the router, so its harness, one
// button that calls the hook, mounts inside a router; the test reads
// where it went. The query client holds a session while `signedIn` says so.
export async function renderEnding(Harness: () => ReactNode, signedIn: boolean) {
  const queryClient = new QueryClient();
  if (signedIn) {
    queryClient.setQueryData(queryKeys.session, SESSION);
  }
  queryClient.setQueryData(queryKeys.mailboxes("a1"), []);
  const rootRoute = createRootRoute({ component: Harness });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByRole("button");
  return { queryClient, router };
}
