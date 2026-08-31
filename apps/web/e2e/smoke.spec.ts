// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "@playwright/test";

test("serves the application shell", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("main")).toBeAttached();
});
