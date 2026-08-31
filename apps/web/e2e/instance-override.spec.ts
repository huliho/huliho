// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

const OVERRIDE_PATH = "/instance/override.css";

const VALID_OVERRIDE = `:root {
  --hh-accent: light-dark(#5b3ea8, #b6a3ec);
  --hh-accent-strong: light-dark(#48318a, #cbbcf2);
  --hh-accent-tint: light-dark(#f2eefb, #251b47);
}`;

const INTERNAL_TOKEN_OVERRIDE = `:root {
  --hhx-row-height: 64px;
}`;

const NESTED_RULE_OVERRIDE = `:root {
  --hh-accent: light-dark(#5b3ea8, #b6a3ec);
  --hh-accent-strong: light-dark(#48318a, #cbbcf2);
  --hh-accent-tint: light-dark(#f2eefb, #251b47);
  & body {
    --hhx-row-height: 64px;
  }
}`;

const DEFAULT_ACCENT = "rgb(30, 118, 136)";

async function serveOverride(page: Page, body: string): Promise<void> {
  await page.route(OVERRIDE_PATH, (route) => route.fulfill({ contentType: "text/css", body }));
}

function rootPropertyValue(page: Page, property: string): Promise<string> {
  return page.evaluate(
    (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim(),
    property,
  );
}

// Reads the color a token actually paints, so light-dark() is resolved.
function paintedColor(page: Page, property: string): Promise<string> {
  return page.evaluate((name) => {
    const probe = document.createElement("span");
    probe.style.color = `var(${name})`;
    document.body.append(probe);
    const painted = getComputedStyle(probe).color;
    probe.remove();
    return painted;
  }, property);
}

const VIOLET_LIGHT = "rgb(91, 62, 168)";
const VIOLET_DARK = "rgb(182, 163, 236)";

test("a valid override rebrands the accent without code changes", async ({ page }) => {
  await serveOverride(page, VALID_OVERRIDE);
  await page.goto("/");
  await expect.poll(() => paintedColor(page, "--hh-accent")).toBe(VIOLET_LIGHT);
  await expect.poll(() => paintedColor(page, "--hhx-toast-accent")).toBe(VIOLET_DARK);
  await page.evaluate(() => {
    document.documentElement.dataset["theme"] = "dark";
  });
  await expect.poll(() => paintedColor(page, "--hh-accent")).toBe(VIOLET_DARK);
  await expect.poll(() => paintedColor(page, "--hhx-toast-accent")).toBe(VIOLET_LIGHT);
});

test("an override hiding a nested rule is rejected entirely", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push(message.text());
    }
  });
  await serveOverride(page, NESTED_RULE_OVERRIDE);
  await page.goto("/");
  await expect
    .poll(() => consoleErrors.find((text) => text.includes(":root rules")) ?? "")
    .toContain("token declarations");
  expect(await rootPropertyValue(page, "--hhx-row-height")).toBe("52px");
  expect(await paintedColor(page, "--hh-accent")).toBe(DEFAULT_ACCENT);
});

test("an override naming an internal token is rejected with a clear message", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push(message.text());
    }
  });
  await serveOverride(page, INTERNAL_TOKEN_OVERRIDE);
  await page.goto("/");
  await expect
    .poll(() => consoleErrors.find((text) => text.includes("--hhx-row-height")) ?? "")
    .toContain("internal token");
  expect(await rootPropertyValue(page, "--hhx-row-height")).toBe("52px");
});
