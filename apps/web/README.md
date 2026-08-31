# @huliho/web

The Huliho web client: a React SPA built with Vite, with the React
Compiler enabled. Design tokens live in `src/styles/tokens.css`; the
`--hh-*` names are the stable instance-override surface and the
`--hhx-*` names are internal. An instance mounts `/instance/override.css`
to rebrand; the app validates it against the stable names and the
contrast floors before applying it.

From the repo root: `pnpm build` builds it, `pnpm test` runs the unit
tests and `pnpm test:e2e` runs the Playwright suite. `pnpm dev` inside
this directory starts the dev server and `pnpm storybook` the component
workshop.

`pnpm lighthouse` audits the built app against the performance and
accessibility budgets in `lighthouserc.cjs`.

The e2e suite screenshots every story in both themes at phone and
desktop width. Baselines are rendered on Linux, so the comparison runs
in CI and the images are refreshed from the repo root with the
container whose tag matches the installed `@playwright/test` version:
`docker run --rm -e CI=true -v "$PWD":/work -w /work mcr.microsoft.com/playwright:v1.62.1-noble bash -c "corepack enable && pnpm install --frozen-lockfile && pnpm --filter @huliho/web exec playwright test --update-snapshots"`
The container swaps `node_modules` to Linux binaries; run
`CI=true pnpm install` afterwards to restore them.
