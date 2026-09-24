// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { defineConfig } from "@playwright/test";

const PREVIEW_PORT = 4173;
const STORYBOOK_PORT = 6007;

export const PREVIEW_URL = `http://localhost:${String(PREVIEW_PORT)}`;
export const STORYBOOK_URL = `http://localhost:${String(STORYBOOK_PORT)}`;

// Screenshot baselines are rendered on Linux, so the comparison only runs in CI
// and in the Linux container that refreshes them (README).
const COMPARE_SCREENSHOTS = process.env.CI !== undefined || process.platform === "linux";

// Baseline names carry the platform and no project name.
const SNAPSHOT_PATH_TEMPLATE =
  "{snapshotDir}/{testFileDir}/{testFileName}-snapshots/{arg}{-snapshotSuffix}{ext}";

// The frame traces run in a project of their own after the rest of the
// suite, so no other worker shares the CPU while one measures.
const SCROLL_BUDGET_SPEC = "**/scroll-budget.spec.ts";

export default defineConfig({
  testDir: "e2e",
  ignoreSnapshots: !COMPARE_SCREENSHOTS,
  snapshotPathTemplate: SNAPSHOT_PATH_TEMPLATE,
  use: {
    baseURL: PREVIEW_URL,
  },
  projects: [
    { name: "suite", testIgnore: SCROLL_BUDGET_SPEC },
    { name: "scroll-budget", testMatch: SCROLL_BUDGET_SPEC, dependencies: ["suite"] },
  ],
  webServer: [
    {
      command: `vite preview --port ${String(PREVIEW_PORT)} --strictPort`,
      port: PREVIEW_PORT,
      reuseExistingServer: false,
      // The app preview sends the server's Content Security Policy.
      env: { HULIHO_PREVIEW_CSP: "1" },
    },
    {
      command: `vite preview --outDir storybook-static --port ${String(STORYBOOK_PORT)} --strictPort`,
      port: STORYBOOK_PORT,
      reuseExistingServer: false,
    },
  ],
});
