// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { PREVIEW_URL } from "../playwright.config";
import { mockSignedOut } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const LINK_KEY = "huliho-link-key";
const DEVICE_KEY = "key-of-this-device";
const TARGET = "https://shop.example.test/sale?x=1";
const MISMATCH = "https://mybank-secure.example.net/login/verify?session=8f3a2c91";

// The address a link of a mail opens: the target, the text the mail
// showed and the key of the device that made the link.
function opens(target: string, text = "", key = ""): string {
  return `/open#${new URLSearchParams({ k: key, u: target, t: text }).toString()}`;
}

// This device made links before: its key is in place when the page starts.
async function withDeviceKey(page: Page): Promise<void> {
  await page.addInitScript(
    ([name, key]) => {
      window.localStorage.setItem(name, key);
    },
    [LINK_KEY, DEVICE_KEY] as const,
  );
}

// Every host a link may lead to answers one plain page; the calls to
// the server are counted, since the route makes none.
async function serveTargets(page: Page): Promise<string[]> {
  const calls: string[] = [];
  await page.route(/^https:\/\/[^/]+\.example\.(?:test|net)\//, (route) =>
    route.fulfill({ contentType: "text/html", body: "<title>target</title><p>the target</p>" }),
  );
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.startsWith("/api/")) {
      calls.push(request.url());
    }
  });
  return calls;
}

test("a plain link this device made leaves for its target at once", async ({ page }) => {
  const calls = await serveTargets(page);
  await withDeviceKey(page);
  await page.goto(opens(TARGET, "See the sale", DEVICE_KEY));
  await expect(page).toHaveURL(TARGET);
  await expect(page.getByText("the target")).toBeVisible();
  expect(calls).toEqual([]);
});

test("a link made elsewhere asks first with Cancel in focus and opens on request", async ({
  page,
}) => {
  const calls = await serveTargets(page);
  await withDeviceKey(page);
  await page.goto(opens(TARGET, "", "a-guess"));
  await expect(
    page.getByRole("heading", { level: 2, name: "This link opens shop.example.test." }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Cancel" })).toBeFocused();
  await expect(page).toHaveURL(/\/open#/);
  await page.getByRole("button", { name: "Open anyway" }).click();
  await expect(page).toHaveURL(TARGET);
  expect(calls).toEqual([]);
});

test("a link whose text names another host is axe-clean and matches its screenshots", async ({
  page,
}) => {
  await withDeviceKey(page);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await page.goto(opens(MISMATCH, "mybank.example", DEVICE_KEY));
        await page.reload();
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        await expect(
          page.getByRole("heading", {
            level: 2,
            name: "This link says mybank.example but opens mybank-secure.example.net.",
          }),
        ).toBeVisible();
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`open-mismatch-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

const SENTENCES = [
  {
    name: "international",
    address: opens("https://xn--mybnk-fsa.example/inloggen", "Sign in", DEVICE_KEY),
    sentence: "This link opens an internationalized domain, xn--mybnk-fsa.example.",
  },
  {
    name: "own",
    address: opens(`${PREVIEW_URL}/settings/accounts`, "Settings", DEVICE_KEY),
    sentence: "This link opens your own mail app at /settings/accounts.",
  },
  {
    name: "invalid",
    address: opens("javascript:top.__x=1", "", DEVICE_KEY),
    sentence: "This link can’t be opened.",
  },
] as const;

test("every other reason has its sentence and its screenshot", async ({ page }) => {
  await withDeviceKey(page);
  for (const { name, address, sentence } of SENTENCES) {
    await test.step(name, async () => {
      await page.goto(address);
      await page.reload();
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      await expect(page.getByRole("heading", { level: 2, name: sentence })).toBeVisible();
      const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
      expect.soft(results.violations, `axe on ${name}`).toEqual([]);
      await expect.soft(page).toHaveScreenshot(`open-${name}-light-desktop.png`, {
        fullPage: true,
      });
    });
  }
});

test("the page reads in Dutch and from right to left", async ({ page }) => {
  await withDeviceKey(page);
  const [phone] = VIEWPORTS;
  await page.setViewportSize({ width: phone.width, height: phone.height });
  for (const locale of ["nl", "en-XA"]) {
    await test.step(locale, async () => {
      await page.addInitScript((value) => {
        window.localStorage.setItem("PARAGLIDE_LOCALE", value);
      }, locale);
      await page.goto(opens(MISMATCH, "mybank.example", DEVICE_KEY));
      await page.reload();
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      await expect(page.getByRole("heading", { level: 2 })).toBeVisible();
      await expect.soft(page).toHaveScreenshot(`open-mismatch-${locale}-light-phone.png`, {
        fullPage: true,
      });
    });
  }
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
});

test("a link inside a sandboxed frame opens the page in a tab of its own, which Cancel closes", async ({
  page,
  context,
}) => {
  await mockSignedOut(page);
  await page.goto("/sign-in");
  await page.evaluate(
    (address) => {
      const frame = document.createElement("iframe");
      frame.setAttribute(
        "sandbox",
        "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
      );
      frame.setAttribute("title", "mail");
      frame.srcdoc = `<a href="${address}" target="_blank" rel="noopener noreferrer">a link</a>`;
      document.body.append(frame);
    },
    opens(MISMATCH, "mybank.example"),
  );
  const opening = context.waitForEvent("page");
  await page.frameLocator('iframe[title="mail"]').getByRole("link", { name: "a link" }).click();
  const tab = await opening;
  await expect(tab.getByRole("button", { name: "Cancel" })).toBeFocused();
  await expect(page).toHaveURL(/\/sign-in$/);
  const closed = tab.waitForEvent("close");
  await tab.getByRole("button", { name: "Cancel" }).click();
  await closed;
});

test("a tab the browser will not close loads the app's start on Cancel", async ({ page }) => {
  await mockSignedOut(page);
  // A tab that came here from another page is not the script's to close.
  await page.goto("/sign-in");
  await page.goto(opens(MISMATCH, "mybank.example"));
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
});
