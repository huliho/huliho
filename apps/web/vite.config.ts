// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { paraglideVitePlugin } from "@inlang/paraglide-js";
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { runnerImport } from "vite";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// The locale follows the system until the user picks one, which sticks.
const paraglide = paraglideVitePlugin({
  project: "../../packages/i18n/project.inlang",
  outdir: "./src/paraglide",
  strategy: ["localStorage", "preferredLanguage", "baseLocale"],
  emitTsDeclarations: true,
});

// The dev server has no API of its own; the running binary answers it.
const API_PROXY_TARGET = "http://localhost:8080";

// The server's policy, sent by the app preview only; Storybook's preview needs its inline scripts.
const CONTENT_SECURITY_POLICY = readFileSync(
  join(import.meta.dirname, "../../crates/server/src/csp.txt"),
  "utf8",
).trim();

// The Lighthouse gate's preview answers the API from the e2e answers; the
// e2e suite's preview leaves it to each test's routes. Vite's module runner
// loads them for that run alone, so no other run bundles them with this file.
const PREVIEW_API =
  process.env.HULIHO_PREVIEW_API === undefined
    ? []
    : [
        (
          await runnerImport<{ previewApi: () => Plugin }>(
            join(import.meta.dirname, "e2e/preview-api.ts"),
          )
        ).module.previewApi(),
      ];

export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] }), paraglide, ...PREVIEW_API],
  server: {
    proxy: {
      "/api": API_PROXY_TARGET,
      "/license": API_PROXY_TARGET,
    },
  },
  preview: {
    headers:
      process.env.HULIHO_PREVIEW_CSP === undefined
        ? {}
        : { "content-security-policy": CONTENT_SECURITY_POLICY },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
