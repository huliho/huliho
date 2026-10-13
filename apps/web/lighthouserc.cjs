// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

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

const ACCESSIBILITY_MIN_SCORE = 1;
const CLS_MAX = 0.05;
// The cold load's requests and bytes: the initial JS budget of 250 KB
// plus the font, the stylesheets and the document. Lighthouse CI counts
// the size in bytes.
const COLD_LOAD_REQUESTS_MAX = 22;
const COLD_LOAD_BYTES_MAX = 300_000;
const PERFORMANCE_MIN_SCORE = 0.95;
const LCP_MAX_MS = 1500;
// The lab stand-in for the INP budget: no long tasks may block input.
const TBT_MAX_MS = 200;

const COUNTED = {
  "categories:accessibility": ["error", { minScore: ACCESSIBILITY_MIN_SCORE }],
  "cumulative-layout-shift": ["error", { maxNumericValue: CLS_MAX }],
  "resource-summary:total:count": ["error", { maxNumericValue: COLD_LOAD_REQUESTS_MAX }],
  "resource-summary:total:size": ["error", { maxNumericValue: COLD_LOAD_BYTES_MAX }],
};
const MEDIAN = { aggregationMethod: "median" };
const TIMED = {
  "categories:performance": ["error", { ...MEDIAN, minScore: PERFORMANCE_MIN_SCORE }],
  "largest-contentful-paint": ["error", { ...MEDIAN, maxNumericValue: LCP_MAX_MS }],
  "total-blocking-time": ["error", { ...MEDIAN, maxNumericValue: TBT_MAX_MS }],
};

module.exports = {
  ci: {
    collect: {
      staticDistDir: "./dist",
      numberOfRuns: TIMING_GATES ? TIMED_RUNS : COUNTED_RUNS,
      settings: {
        throttling: {
          cpuSlowdownMultiplier: MID_RANGE_CPU_SLOWDOWN,
          rttMs: MID_RANGE_RTT_MS,
          throughputKbps: MID_RANGE_THROUGHPUT_KBPS,
        },
      },
    },
    assert: {
      assertions: TIMING_GATES ? { ...COUNTED, ...TIMED } : COUNTED,
    },
  },
};
