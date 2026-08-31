// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { defineConfig } from "@playwright/test";

const PREVIEW_PORT = 4173;
const PREVIEW_URL = `http://localhost:${String(PREVIEW_PORT)}`;

export default defineConfig({
  testDir: "e2e",
  use: {
    baseURL: PREVIEW_URL,
  },
  webServer: {
    command: `vite preview --port ${String(PREVIEW_PORT)} --strictPort`,
    port: PREVIEW_PORT,
    reuseExistingServer: false,
  },
});
