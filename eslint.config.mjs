// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Second lint layer: the two rule sets the primary linter lacks.
// No core rules here, so nothing runs twice.
import parser from "@typescript-eslint/parser";
import security from "eslint-plugin-security";
import sonarjs from "eslint-plugin-sonarjs";

const COGNITIVE_COMPLEXITY_MAX = 12;

export default [
  {
    ignores: [
      "**/dist/**",
      "**/target/**",
      "**/.turbo/**",
      "**/coverage/**",
      "**/storybook-static/**",
      "apps/web/src/paraglide/**",
    ],
  },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: { parser },
    plugins: { security, sonarjs },
    rules: {
      ...security.configs.recommended.rules,
      "sonarjs/cognitive-complexity": ["error", COGNITIVE_COMPLEXITY_MAX],
    },
  },
];
