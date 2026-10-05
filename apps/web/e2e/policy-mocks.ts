// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BrowserContext } from "@playwright/test";

const LIST_ROUTE = "**/api/sender-policies";
const ROW_ROUTE = "**/api/sender-policies/**";
const ROUTE_PATH = "/api/sender-policies/";

// One policy as the server lists it.
interface SenderPolicyBody {
  sender: string;
  key: string;
  value: unknown;
}

// The policies of one user, by sender. Two browser contexts that share
// a store share the user's grants, as two devices on one session do.
export type PolicyStore = Map<string, SenderPolicyBody>;

// The sender a row route names, decoded as the server would.
function senderOf(url: string): string {
  const path = new URL(url).pathname;
  const [sender = ""] = path.slice(path.indexOf(ROUTE_PATH) + ROUTE_PATH.length).split("/");
  return decodeURIComponent(sender);
}

function isWrite(value: unknown): value is { key: string; value: unknown } {
  return (
    typeof value === "object" && value !== null && typeof Reflect.get(value, "key") === "string"
  );
}

// Answers the sender policies for every page of the context: the list
// from the store, a grant written into it, a removal taken out of it.
export async function mockPolicies(context: BrowserContext, store: PolicyStore): Promise<void> {
  await context.route(LIST_ROUTE, (route) =>
    route.request().method() === "GET"
      ? route.fulfill({ json: [...store.values()] })
      : route.fulfill({ status: 405 }),
  );
  await context.route(ROW_ROUTE, (route) => {
    const method = route.request().method();
    const sender = senderOf(route.request().url());
    const body: unknown = method === "PUT" ? route.request().postDataJSON() : null;
    if (method === "PUT" && isWrite(body)) {
      store.set(sender, { sender, key: body.key, value: body.value });
    } else if (method === "DELETE") {
      store.delete(sender);
    }
    return route.fulfill({ status: 204 });
  });
}
