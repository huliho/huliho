// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";

const PREFERENCES_ROUTE = "**/api/preferences";
const PREFERENCE_ROUTE = "**/api/preferences/*";
const FORCED_BODY = { error: "password_change_required" };

export interface PreferenceWrite {
  key: string;
  value: unknown;
}

function isWrite(value: unknown): value is { value: unknown } {
  return typeof value === "object" && value !== null && "value" in value;
}

// Answers the preferences as the server would: the words on record, and
// every write recorded and folded into the next answer. `writeStatus`
// lets a test refuse the writes instead.
export async function mockPreferences(
  page: Page,
  initial: Record<string, string> = {},
  writeStatus = 204,
): Promise<{ writes: PreferenceWrite[] }> {
  const current = new Map(Object.entries(initial));
  const writes: PreferenceWrite[] = [];
  await page.route(PREFERENCES_ROUTE, (route) =>
    route.request().method() === "GET"
      ? route.fulfill({ json: Object.fromEntries(current) })
      : route.fulfill({ status: 405 }),
  );
  await page.route(PREFERENCE_ROUTE, (route) => {
    const key = route.request().url().split("/").pop() ?? "";
    const body: unknown = route.request().postDataJSON();
    const value = isWrite(body) ? body.value : undefined;
    writes.push({ key, value });
    if (writeStatus === 204 && typeof value === "string") {
      current.set(key, value);
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ status: writeStatus, json: { error: "invalid_request" } });
  });
  return { writes };
}

// Refuses the words on record while `forced()` holds, as the server does
// for a session opened with a one-time password; the mock behind answers
// once it lets go.
export async function refusePreferencesWhile(page: Page, forced: () => boolean): Promise<void> {
  await page.route(PREFERENCES_ROUTE, (route) =>
    forced() ? route.fulfill({ status: 403, json: FORCED_BODY }) : route.fallback(),
  );
}
