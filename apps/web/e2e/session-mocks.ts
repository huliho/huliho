// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";

const SESSION_ROUTE = "**/api/session";

const SESSION_BODY = {
  user: { id: "user-1", login: "mira@example.com", role: "owner" },
  organization: { id: "org-1", name: "mira@example.com" },
};

const SIGNED_OUT_BODY = { error: "unauthenticated" };

export async function mockSignedIn(page: Page): Promise<void> {
  await page.route(SESSION_ROUTE, (route) => {
    if (route.request().method() === "GET") {
      return route.fulfill({ json: SESSION_BODY });
    }
    return route.fulfill({ status: 204 });
  });
}

export async function mockSignedOut(page: Page): Promise<void> {
  await page.route(SESSION_ROUTE, (route) => route.fulfill({ status: 401, json: SIGNED_OUT_BODY }));
}

// One page-scoped account: any credentials sign in, sign-out signs out.
export async function mockSessionFlow(page: Page): Promise<void> {
  let signedIn = false;
  await page.route(SESSION_ROUTE, (route) => {
    const method = route.request().method();
    if (method === "POST") {
      signedIn = true;
      return route.fulfill({ status: 204 });
    }
    if (method === "DELETE") {
      signedIn = false;
      return route.fulfill({ status: 204 });
    }
    if (signedIn) {
      return route.fulfill({ json: SESSION_BODY });
    }
    return route.fulfill({ status: 401, json: SIGNED_OUT_BODY });
  });
}
