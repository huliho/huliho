// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { paraglideVitePlugin } from "@inlang/paraglide-js";
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The locale follows the system until the user picks one, which sticks.
const paraglide = paraglideVitePlugin({
  project: "../../packages/i18n/project.inlang",
  outdir: "./src/paraglide",
  strategy: ["localStorage", "preferredLanguage", "baseLocale"],
  emitTsDeclarations: true,
});

export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] }), paraglide],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
