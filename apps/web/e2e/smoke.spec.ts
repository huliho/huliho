// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "@playwright/test";

import { mockSignedIn, mockSignedOut } from "./session-mocks";

test("serves the application shell", async ({ page }) => {
  await mockSignedIn(page);
  await page.goto("/");
  await expect(page.getByRole("main")).toBeAttached();
});

test("serves the sign-in screen when no session exists", async ({ page }) => {
  await mockSignedOut(page);
  await page.goto("/");
  await expect(page.getByLabel("Name")).toBeVisible();
});
