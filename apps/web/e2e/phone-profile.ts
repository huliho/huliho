// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { pageFunctions } from "lighthouse/core/lib/page-functions.js";

// From an input to the painted answer.
export const INTERACTION_BUDGET_MS = 100;
// The middle of the high-end mobile bracket, 800 to 1200, in Lighthouse's
// docs/throttling.md. The phone profile slows the CPU by the host's own
// index, measured before throttling, over this one, so the profile means
// the same on every host.
const MID_RANGE_PHONE_BENCHMARK_INDEX = 1000;
// No slowdown: the floor, for a host no faster than the phone.
export const NO_SLOWDOWN = 1;

// How fast the host is and how far a profile slows it.
export interface HostSpeed {
  benchmarkIndex: number;
  slowdown: number;
}

export function benchmarkIndexOf(page: Page): Promise<number> {
  return page.evaluate(pageFunctions.computeBenchmarkIndex);
}

// Runs `measure` with the page's CPU slowed to the phone profile.
export async function onPhoneProfile(
  page: Page,
  measure: (speed: HostSpeed) => Promise<void>,
): Promise<void> {
  const benchmarkIndex = await benchmarkIndexOf(page);
  const slowdown = Math.max(NO_SLOWDOWN, benchmarkIndex / MID_RANGE_PHONE_BENCHMARK_INDEX);
  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setCPUThrottlingRate", { rate: slowdown });
  await measure({ benchmarkIndex, slowdown });
  await session.send("Emulation.setCPUThrottlingRate", { rate: NO_SLOWDOWN });
}
