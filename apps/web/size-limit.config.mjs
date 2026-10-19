// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The size budgets of the build in dist. The initial JS is what the page
// loads at its start: every script index.html names and every chunk it
// preloads.
import { globSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { JSDOM } from "jsdom";

const INITIAL_JS_LIMIT = "250 kB";
const LAZY_CHUNK_LIMIT = "100 kB";

// Each chunk the page loads after its start carries its module's name.
const LAZY_CHUNKS = [
  ["message inspector chunk", "message-inspector"],
  ["open route chunk", "open-link"],
  ["command overlays chunk", "overlays"],
  ["thread pane chunk", "thread-pane"],
  ["menus chunk", "account-menu"],
  ["settings chunk", "pages"],
  ["add-account chunk", "add-account"],
  ["cache worker", "worker"],
];

const html = readFileSync(join(import.meta.dirname, "dist", "index.html"), "utf8");
const { document } = new JSDOM(html).window;
const start = [
  ...Array.from(document.querySelectorAll("script[src]"), (script) => script.getAttribute("src")),
  ...Array.from(document.querySelectorAll("link[rel~=modulepreload][href]"), (link) =>
    link.getAttribute("href"),
  ),
].map((url) => join("dist", url));

const lazy = LAZY_CHUNKS.map(([name, module]) => ({
  name,
  path: `dist/assets/${module}-*.js`,
  limit: LAZY_CHUNK_LIMIT,
  gzip: true,
}));

// Every other chunk loads after the start too: the code the lazy chunks
// share and any new chunk that has no entry above.
const shared = globSync("dist/assets/*.js", {
  cwd: import.meta.dirname,
  exclude: [...start, ...lazy.map(({ path }) => path)],
});

export default [
  { name: "initial JS", path: start, limit: INITIAL_JS_LIMIT, gzip: true },
  ...lazy,
  ...(shared.length > 0
    ? [{ name: "shared chunks", path: shared, limit: LAZY_CHUNK_LIMIT, gzip: true }]
    : []),
];
