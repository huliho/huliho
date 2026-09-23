# @huliho/web

The Huliho web client: a React SPA built with Vite, with the React
Compiler enabled. Design tokens live in `src/styles/tokens.css`; the
`--hh-*` names are the stable instance-override surface and the
`--hhx-*` names are internal. An instance mounts `/instance/override.css`
to rebrand; the app validates it against the stable names and the
contrast floors before applying it.

Strings come from the `@huliho/i18n` catalogs through Paraglide; the
Vite plugin compiles them into `src/paraglide/`, a generated directory
that typecheck and unit tests read, so a fresh clone runs `pnpm build`
once before either. The test script disables Node's own experimental
localStorage global, which would otherwise shadow the jsdom one that
the locale strategy reads.

Single-key commands (Escape closes settings, `z` undoes) come from one
registry in `src/commands`; the root layout installs its listener and
hosts the toast viewport, so a toast outlives the route that opened it.
A revoke on the sessions page leaves the list at once and reaches the
server only when its undo toast has run out. If the page closes first,
the request goes out on page hide with `keepalive`.

Sign-in and the password change run through one credential hook: a
rate-limited refusal holds the form and counts down. A session opened
with a one-time password reaches only `/choose-password`; the router
sends every other route there until the change lands, then back to `/`.

Admin > Users lists the organization's users for admins and owners; a
member is sent to the sessions page and never sees the Admin group.
Reset and create hand over a one-time password in a dialog that shows
it once: it lives in mutation state until the dialog has closed, then
both mutations reset.

Adding a mail account is a card at `/accounts/new`, where a session
without accounts lands from the shell. The flow is a reducer over the
steps (typing, detecting, found, confirm host, not found, manual,
connecting, insecure, consent, consent denied) with the fields of each
step in plain component state; four requests sit behind it (discover,
add, replace a credential, start a consent) and every refusal is one
sentence on the field or above the fields. A reconnect opens the same
card at
`/accounts/new?reconnect={id}` with the address fixed. The address is
checked against the server's shape rule before it goes out. A
credential travels once, at Connect; no cache ever holds it.
A Google or Microsoft account signs in through a consent: the session
lists the providers the instance can start one with, the card opens
the provider in a window it holds no opener to, polls the outcome
every two seconds and ends in the toast or in one sentence saying why
nothing was connected. An OAuth row reconnects the same way.

Settings > Accounts lists the connected accounts, one row each with
its state: nothing while connected, Connection expired with Reconnect
once the upstream rejected the credential, the stopped sentence with
Retry once a run of refused connections tripped the stop. Retry sends
the row id and says on the row what came of it; Remove takes the row
out at once and reaches the server when its undo toast has run out,
through the same deferred mutation as a revoke. A connect from the
card lands here with its toast.

Settings > Appearance holds the theme, the density, the reading pane
position and the language, each a row of segments on the preferences
the server keeps per user. A choice shows on the screen first and
reaches the server next; a refused save puts the word on record back
and says so. Every route behind a session guard renders inside one
layout, which applies what the server holds: the theme and the density
as attributes on the document, the language on every mounted screen
without a reload, since every screen reads the locale through one
store in `src/i18n`. A key the user never chose applies its default:
the device's own scheme, comfortable rows, the reading pane on the
right and the browser's language. The device remembers the last theme
and density it applied, so the next load starts there before the
server answers. While a one-time password is in force the server holds
the words back, so the forced step keeps the device's own; they apply
once the change lands. The pseudo locale is a development entry that
never reaches the server.

The mail cache runs in a worker under `src/cache`: a shared worker
(one per origin) or a dedicated worker per tab where the browser has
none. It keeps the mailbox tree, the headers, the threads and the
pages of each list in IndexedDB through Dexie: one database per origin
with every row under its account id. Each account reconciles under a
Web Lock, so two workers on one database never interleave. The shell
tells the worker which accounts the session holds and which mailbox
the tab is looking at, renewed every thirty seconds while the tab
lives; the newest list wins, so a tab that is behind never undoes
one. The worker fetches the tree when an account arrives, asks for
changes every minute and when a tab comes back into view and tells
every tab over a broadcast channel what changed, which invalidates
the queries by their keys. The window asks the browser for persistent
storage once per page and the worker holds every write until that
request has an answer. A failure crosses the worker boundary as a
result with its code and cause, so the shell can tell a stopped
account from a server that did not answer. Sign-out deletes the
database; every other tab of the session hears it and shows the
sign-in screen with a word about it.

The mail screen lives under `src/mail` at `/mail/{accountId}/{mailboxId}`.
The root sends a visit to the account this device opened last, else to
the oldest one. An account named without a mailbox opens its inbox, or
the first mailbox it has. The shell has three layouts on two
breakpoints, 720 and 1200 CSS pixels: below the first a phone shows the
list alone and opens the sidebar as a sheet from the avatar in its
header; from there a tablet shows the sidebar as a rail with the roles
and a button for the whole tree; from the second the sidebar, the list
and the reading pane stand side by side, with a seam between list and
reading pane that drags, answers the arrow keys, resets on Enter or a
double click and keeps its place on the device. The sidebar holds the
account switcher (a menu with every account of the session, Settings
and Sign out) over the mailbox tree: the six roles in a fixed order,
then the folders with their depth. The tree is one tab stop: the arrow
keys move inside it and Tab leaves it. Mailbox names are the server's
words; the counts are unread mail, and drafts for the drafts folder.

From the repo root: `pnpm build` builds it, `pnpm test` runs the unit
tests and `pnpm test:e2e` runs the Playwright suite. `pnpm dev` inside
this directory starts the dev server and `pnpm storybook` the component
workshop.

`pnpm lighthouse` audits the built app against the performance and
accessibility budgets in `lighthouserc.cjs`.

The app preview sends the same Content Security Policy as the server,
read from `crates/server/src/csp.txt`, so every e2e test against the
app runs under it.
The Storybook preview goes without, since its bootstrap uses inline
scripts.

The e2e suite screenshots every story and the served page in both
themes at phone and desktop width. It serves the built app and the
built Storybook, so a stale build gives stale images. Baselines are
rendered on Linux, so the comparison runs in CI and the images are
refreshed from the repo root with the container whose tag matches the
installed `@playwright/test` version, building both first:
`docker run --rm -e CI=true -v "$PWD":/work -w /work mcr.microsoft.com/playwright:v1.63.0-noble bash -c "corepack enable && pnpm install --frozen-lockfile && pnpm build && pnpm --filter @huliho/web build:storybook && pnpm --filter @huliho/web exec playwright test --update-snapshots"`
The container swaps `node_modules` to Linux binaries; run
`CI=true pnpm install` afterwards to restore them.
