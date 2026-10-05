// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockPrivacy } from "./session-mocks";

// The database the cache worker keeps mail in.
const DATABASE = "huliho-mail";

function databases(page: Page): Promise<string[]> {
  return page.evaluate(async () => (await indexedDB.databases()).map((row) => row.name ?? ""));
}

async function shown(page: Page): Promise<void> {
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  await expect(page.getByRole("treeitem").first()).toBeVisible();
}

test("an instance that turns strict keeps no mail on disk from the next load on", async ({
  page,
}) => {
  const turn = await mockPrivacy(page);
  await page.goto("/");
  await shown(page);
  await expect.poll(() => databases(page)).toContain(DATABASE);
  turn(true);
  await page.reload();
  await shown(page);
  await expect.poll(() => databases(page)).not.toContain(DATABASE);
  await page.reload();
  await shown(page);
  expect(await databases(page)).not.toContain(DATABASE);
});
