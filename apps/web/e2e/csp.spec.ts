// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockSessionFlow, mockSessions, sessionRows } from "./session-mocks";

const ATTRIBUTION = "Huliho, by Eric Kochen";
const FIXED_NOW = new Date("2026-05-14T10:00:00");

// The listener lands before any script of the page, so a violation during
// startup is caught too; every navigation starts a fresh list.
async function recordViolations(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const recorded: string[] = [];
    Object.assign(window, { __cspViolations: recorded });
    document.addEventListener("securitypolicyviolation", (event) => {
      recorded.push(
        `${event.effectiveDirective} blocked ${event.blockedURI} at ${event.sourceFile}:${String(event.lineNumber)}`,
      );
    });
  });
}

function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

async function violations(page: Page): Promise<string[]> {
  const found = await page.evaluate(
    (): unknown => Reflect.get(window, "__cspViolations") as unknown,
  );
  return isStringList(found) ? found : ["the violation list is missing"];
}

test("the app runs under the server's policy without a violation", async ({ page }) => {
  await recordViolations(page);
  await mockSessionFlow(page);
  const response = await page.goto("/sign-in");
  expect(response?.headers()["content-security-policy"]).toContain("default-src 'self'");
  await expect(page.getByLabel("Name")).toBeVisible();
  expect(await violations(page)).toEqual([]);

  await page.getByLabel("Name").fill("mira@example.com");
  await page.getByLabel("Password").fill("example passphrase");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("button", { name: "Sign out" })).toBeVisible();
  expect(await violations(page)).toEqual([]);

  await page.goto("/settings/about");
  await expect(page.getByText(ATTRIBUTION)).toBeVisible();
  expect(await violations(page)).toEqual([]);

  // The toast positions itself through the CSSOM, which the policy does not govern.
  await mockSessions(page, sessionRows(FIXED_NOW));
  await page.goto("/settings/sessions");
  await page.getByRole("button", { name: "Revoke Safari on macOS" }).click();
  await expect(page.getByText("Safari session revoked.")).toBeVisible();
  expect(await violations(page)).toEqual([]);
});
