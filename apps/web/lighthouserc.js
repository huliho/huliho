// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Lighthouse CI loads this file with require, which takes an ES module in
// the pinned Node and reads its `ci` export.

import { chromium } from "@playwright/test";

// The mid-range phone profile the budgets are measured on: a 4x slowed CPU
// on a 10 Mbps link with 40 ms round trips.
const MID_RANGE_CPU_SLOWDOWN = 4;
const MID_RANGE_RTT_MS = 40;
const MID_RANGE_THROUGHPUT_KBPS = 10240;

// The timed assertions run only while this variable is set, as the timing
// job of ci.yml does, over the median of five runs; otherwise one run
// asserts what a host's load cannot move.
const TIMING_GATES = process.env.HULIHO_TIMING_GATES !== undefined;
const TIMED_RUNS = 5;
const COUNTED_RUNS = 1;

// The app preview with the server's policy and an API answered from the
// e2e answers, on a port of its own beside the e2e suite's.
const PREVIEW_PORT = 4174;
const ORIGIN = `http://localhost:${String(PREVIEW_PORT)}`;
// Vite prints this label once it listens, with or without color.
const SERVER_READY_PATTERN = "Local";
// The sign-in screen without a session and the inbox of the account the
// preview's API lists with one. Lighthouse CI matches each page by the
// address its document loaded from, not where the app routes it after.
const SIGN_IN_PATH = "/sign-in";
const MAIL_PATH = "/mail/acc-0/mb-inbox";
// Lighthouse's own list plus the stores the mail cache writes, so every
// run is a first load; the session cookie stays.
const CLEARED_STORAGE = [
  "file_systems",
  "shader_cache",
  "service_workers",
  "cache_storage",
  "indexeddb",
  "local_storage",
];

const ACCESSIBILITY_MIN_SCORE = 1;
const CLS_MAX = 0.05;
// The sign-in screen's cold load: the initial JS budget of 250 KB plus
// the font, the stylesheets, the document and the session's answer.
// Lighthouse CI counts the size in bytes.
const COLD_LOAD_REQUESTS_MAX = 22;
const COLD_LOAD_BYTES_MAX = 300_000;
// The mail route's load: the start, the shell's chunks and fonts and the
// account's answers, with a small margin.
const MAIL_LOAD_REQUESTS_MAX = 42;
const MAIL_LOAD_BYTES_MAX = 465_000;
const PERFORMANCE_MIN_SCORE = 0.95;
const LCP_MAX_MS = 1500;
// The lab stand-in for the INP budget: no long tasks may block input.
const TBT_MAX_MS = 200;

const MEDIAN = { aggregationMethod: "median" };
const TIMED = {
  "categories:performance": ["error", { ...MEDIAN, minScore: PERFORMANCE_MIN_SCORE }],
  "largest-contentful-paint": ["error", { ...MEDIAN, maxNumericValue: LCP_MAX_MS }],
  "total-blocking-time": ["error", { ...MEDIAN, maxNumericValue: TBT_MAX_MS }],
};

/**
 * @param {number} requestsMax
 * @param {number} bytesMax
 */
function assertionsFor(requestsMax, bytesMax) {
  const counted = {
    "categories:accessibility": ["error", { minScore: ACCESSIBILITY_MIN_SCORE }],
    "cumulative-layout-shift": ["error", { maxNumericValue: CLS_MAX }],
    "resource-summary:total:count": ["error", { maxNumericValue: requestsMax }],
    "resource-summary:total:size": ["error", { maxNumericValue: bytesMax }],
  };
  return TIMING_GATES ? { ...counted, ...TIMED } : counted;
}

export const ci = {
  collect: {
    startServerCommand: `HULIHO_PREVIEW_CSP=1 HULIHO_PREVIEW_API=1 vite preview --port ${String(PREVIEW_PORT)} --strictPort`,
    startServerReadyPattern: SERVER_READY_PATTERN,
    url: [`${ORIGIN}${SIGN_IN_PATH}`, `${ORIGIN}${MAIL_PATH}`],
    puppeteerScript: "./lighthouse-session.cjs",
    chromePath: chromium.executablePath(),
    // Playwright starts the same Chromium without its sandbox, which does
    // not start as root in a container.
    puppeteerLaunchOptions: { args: ["--no-sandbox"] },
    numberOfRuns: TIMING_GATES ? TIMED_RUNS : COUNTED_RUNS,
    settings: {
      clearStorageTypes: CLEARED_STORAGE,
      throttling: {
        cpuSlowdownMultiplier: MID_RANGE_CPU_SLOWDOWN,
        rttMs: MID_RANGE_RTT_MS,
        throughputKbps: MID_RANGE_THROUGHPUT_KBPS,
      },
    },
  },
  assert: {
    assertMatrix: [
      {
        matchingUrlPattern: `${SIGN_IN_PATH}$`,
        assertions: assertionsFor(COLD_LOAD_REQUESTS_MAX, COLD_LOAD_BYTES_MAX),
      },
      {
        matchingUrlPattern: `${MAIL_PATH}$`,
        assertions: assertionsFor(MAIL_LOAD_REQUESTS_MAX, MAIL_LOAD_BYTES_MAX),
      },
    ],
  },
};
