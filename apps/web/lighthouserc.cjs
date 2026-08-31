// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mid-range phone profile the budgets are measured on: a 4x slowed CPU
// on a 10 Mbps link with 40 ms round trips.
const MID_RANGE_CPU_SLOWDOWN = 4;
const MID_RANGE_RTT_MS = 40;
const MID_RANGE_THROUGHPUT_KBPS = 10240;

const RUNS = 3;
const PERFORMANCE_MIN_SCORE = 0.95;
const ACCESSIBILITY_MIN_SCORE = 1;
const LCP_MAX_MS = 1500;
const CLS_MAX = 0.05;
// The lab stand-in for the INP budget: no long tasks may block input.
const TBT_MAX_MS = 200;

module.exports = {
  ci: {
    collect: {
      staticDistDir: "./dist",
      numberOfRuns: RUNS,
      settings: {
        throttling: {
          cpuSlowdownMultiplier: MID_RANGE_CPU_SLOWDOWN,
          rttMs: MID_RANGE_RTT_MS,
          throughputKbps: MID_RANGE_THROUGHPUT_KBPS,
        },
      },
    },
    assert: {
      assertions: {
        "categories:performance": ["error", { minScore: PERFORMANCE_MIN_SCORE }],
        "categories:accessibility": ["error", { minScore: ACCESSIBILITY_MIN_SCORE }],
        "largest-contentful-paint": ["error", { maxNumericValue: LCP_MAX_MS }],
        "cumulative-layout-shift": ["error", { maxNumericValue: CLS_MAX }],
        "total-blocking-time": ["error", { maxNumericValue: TBT_MAX_MS }],
      },
    },
  },
};
