// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail, Preferences } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import type { JSX } from "react";

import { previewDetail } from "./body-fixtures";
import { FASTMAIL, THREAD } from "./fixtures";

// The links and the queries of a mail story need a router and a query
// client; a memory router at the shell's own address serves. `seed`
// fills the client with what the story's server would answer.
export function routed(
  Screen: () => JSX.Element,
  path = "/mail/acc-1/mb-inbox",
  seed: (client: QueryClient) => void = () => undefined,
): JSX.Element {
  const router = createRouter({
    routeTree: createRootRoute({ component: Screen }),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient();
  seed(client);
  return (
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

// What an open card reads, in place before the first render so a story
// draws its message at once: no policy on record, the preferences given
// (none unless a story sets one) and the body of every message of the
// fixture thread, its preview unless `bodies` holds one.
export function seedMail(
  bodies: ReadonlyMap<string, BodyDetail>,
  preferences: Preferences = {},
): (client: QueryClient) => void {
  return (client) => {
    client.setQueryData(queryKeys.senderPolicies, []);
    client.setQueryData(queryKeys.preferences, preferences);
    for (const header of Object.values(THREAD.emails)) {
      client.setQueryData(
        queryKeys.body(FASTMAIL.id, header.id),
        bodies.get(header.id) ?? previewDetail(header),
      );
    }
  };
}
