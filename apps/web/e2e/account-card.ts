// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect } from "@playwright/test";

import { mockAccounts } from "./account-mocks";
import type { AccountRowBody, AccountsAnswers, Recorded } from "./account-mocks";
import { mockMail } from "./mail-mocks";
import { mockSignedIn } from "./session-mocks";
import type { MockSignIn } from "./session-mocks";

// Screenshots must not age, so the page renders a pinned date.
export const FIXED_NOW = new Date("2026-05-14T10:00:00");

// What the acceptance criteria record per provider: the steps the user
// acted on and the fields the test typed into.
export interface Walk {
  stops: string[];
  typed: string[];
}

export function card(page: Page): Locator {
  return page.locator("[data-step]");
}

export function field(page: Page, label: string): Locator {
  // The outgoing server repeats the incoming labels; the first is the incoming one.
  return page.getByLabel(label, { exact: true }).first();
}

export async function openCard(
  page: Page,
  answers: AccountsAnswers = {},
  rows: AccountRowBody[] = [],
  signInProviders: MockSignIn[] = [],
): Promise<Recorded> {
  await mockSignedIn(page, "owner", signInProviders);
  const recorded = await mockAccounts(page, rows, answers);
  // A connect lands in the new account's inbox, which needs its mail.
  await mockMail(page);
  await page.clock.install({ time: FIXED_NOW });
  await page.goto("/accounts/new");
  await expect(page.getByRole("heading", { level: 1, name: "Add a mail account" })).toBeVisible();
  return recorded;
}

export async function stopAt(page: Page, walk: Walk, step: string): Promise<void> {
  await expect(card(page)).toHaveAttribute("data-step", step);
  walk.stops.push(step);
}

export async function typeInto(
  page: Page,
  walk: Walk,
  label: string,
  value: string,
): Promise<void> {
  await field(page, label).fill(value);
  walk.typed.push(label);
}
