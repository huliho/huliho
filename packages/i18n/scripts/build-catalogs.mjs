// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Runs from the compiled package, so the root build must come first.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  catalogIssues,
  parseCatalog,
  parseSettings,
  PSEUDO_LOCALE,
  pseudoCatalog,
} from "@huliho/i18n";

const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));

/** @param {...string} segments */
function readJson(...segments) {
  const path = join(packageRoot, ...segments);
  return { path, value: /** @type {unknown} */ (JSON.parse(readFileSync(path, "utf8"))) };
}

const settingsFile = readJson("project.inlang", "settings.json");
const settings = parseSettings(settingsFile.value, settingsFile.path);
const catalogs = Object.fromEntries(
  settings.locales
    .filter((locale) => locale !== PSEUDO_LOCALE)
    .map((locale) => {
      const file = readJson("messages", `${locale}.json`);
      return [locale, parseCatalog(file.value, file.path)];
    }),
);

const issues = catalogIssues(settings.baseLocale, catalogs);
if (issues.length > 0) {
  for (const issue of issues) {
    console.error(issue);
  }
  console.error("the message catalogs are out of sync; a missing translation fails the build");
  process.exit(1);
}

const pseudo = pseudoCatalog(catalogs[settings.baseLocale] ?? {});
writeFileSync(
  join(packageRoot, "messages", `${PSEUDO_LOCALE}.json`),
  `${JSON.stringify(pseudo, null, 2)}\n`,
);
