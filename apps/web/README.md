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

Every keyboard command comes from one registry in `src/commands`: a
command carries its keys, a label and a group. The keys are one key, a
combination on the platform's command modifier or `g` followed by a
letter, which the registry waits one second for. The root layout
installs the key listener and hosts the toast viewport, so a toast
outlives the route that opened it. Single keys never fire while typing;
a dialog or a menu keeps every key to itself. Behind every signed-in
screen stand the command palette (Cmd/Ctrl+K) and the shortcut overlay
(`?`). The palette lists every registered command by group with the
ones last run first, narrows on a query and runs the highlighted one
once it has closed and given the focus back; the overlay lists the
same commands with their keys. Both come as one chunk fetched when the
layout mounts.
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
step in plain component state; five requests sit behind it (discover,
add, replace a credential, start a consent, end a consent) and every
refusal is one sentence on the field or above the fields. A reconnect opens the same
card at
`/accounts/new?reconnect={id}` with the address fixed. The address is
checked against the server's shape rule before it goes out. A
credential travels once, at Connect; no cache ever holds it.
A Google or Microsoft account signs in through a consent: the session
lists the providers the instance can start one with, the card opens
the provider in a window it holds no opener to, polls the outcome
every two seconds and ends in the toast or in one sentence saying why
nothing was connected. Cancel ends the consent on the server, so a
window still at the provider lands nothing. An OAuth row reconnects the
same way. A first add lands in the new account's inbox with its toast;
a reconnect returns to where the card was opened, the mail or the
accounts page.

Settings > Accounts lists the connected accounts, one row each with
its state: nothing while connected, Connection expired with Reconnect
once the upstream rejected the credential, the stopped sentence with
Retry once a run of refused connections tripped the stop. Retry sends
the row id and says on the row what came of it; Remove takes the row
out at once and reaches the server when its undo toast has run out,
through the same deferred mutation as a revoke.

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
with every row under its account id. The database's second version
adds a table for message bodies and one for the changes the server has
not acknowledged. The worker opens no store until a tab names the
instance's privacy setting from the session answer: on an instance
that keeps mail off the disk it holds the rows in memory and deletes a
database an earlier start left. A worker that stored on disk when a
tab names that setting starts over in memory and every tab reads its
mail again; from then on it stays in memory, whatever a tab with an
older answer names. A worker that keeps mail off the disk says so to
every tab and a tab whose session answer says otherwise reads that
answer again. Each account reconciles under a
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

The worker also answers the body of one message. It reads a body
outside the account's lock, so opening a message waits for no poll. A
change to a message goes through the worker as well: the rows take it
at once and every tab hears it; the server gets it within 300 ms,
ahead of every poll and when the network comes back. A change the
account or a mailbox does not allow moves nothing. One the server
refuses is taken back and the tab in view says so.

A card that opens shows its message under a hairline: HTML in the
frame the pipeline below builds, titled by who wrote and what about,
plain text with each quote level as a quote of its own behind a bar
and every address a link through the `/open` route. The frame takes
its type and colors from the card. Three still lines stand in for a
body on its way and the message shows once its document is parsed,
ahead of its images. A body the server refuses offers Try again.
Offline the card shows what the device holds and says so for a message
it never got. A body that arrived cut short offers the whole message
at a larger cap and the download when that comes back cut as well. A
plain message draws fifty thousand lines at most and says so under the
last, with the download. A card marks its message read as it opens
unread; a message that turns unread under an open card stays unread. A
message that lands in the open thread comes in folded and stays unread
until its head opens it.
The card lays its head out by its own width, so it reads the same in a
narrow pane as on a phone.

The head of an open card offers the message's details, as does the
palette: a dialog with the receiving server's verdict in one sentence
above three tabs. Rendered shows the message as the card does, Plain
text its text part or that there is none and Source the first 512 kB
of the raw message as the server holds it, unwrapped, with a notice
where the message runs past that; offline the source says what the
body says. The download of the whole message stands in the dialog's
foot. On a phone the dialog fills the screen.

What a message carries stands in a strip under its body: a chip per
attachment with its icon, its name and its size, a raster image shown
in place and opened at full size in a dialog, an attached message that
saves as a file of its own. A chip downloads through the server's
route, which decides what the bytes are. A name that can run a program
asks first and keeps the focus on the chip after either answer.

A message that names remote images gets a bar above it. Load once
loads them for this view. Always for this sender writes a grant pinned
to what the receiving server said about the message and Stop takes it
back; a later message that fails that check keeps its images blocked
and says why. A button of the card that its own press takes away hands
its focus to the bar or the body it stood in. In the dark theme a
light-only message is adapted and its head offers to show it as sent.
The worker fetches bodies two at a time per account: a request the
server holds back for its limit goes on the queue again, five times at
most before the card shows the fault. An account that leaves takes its
waiting fetches with it.

The body pipeline under `src/mail/body` builds the document a message
renders from. DOMPurify runs the same allowlist as the server's
sanitizer. The message's CSS is read through the browser's own parser:
an import, a font and a viewport length leave. One policy settles
every image: a part of the message loads from the download route and a
remote image through the server's proxy once the reader allowed the
sender; until then a box with its alt text stands in its place. A
message draws a hundred such boxes and keeps a blocked image past them
blank; a mail of twenty thousand images stands about three seconds
after the click on a desktop and thirteen on the phone profile. The
app's theme decides the color scheme inside the message, a light-only
message can be adapted for the dark theme and every link points at the
app's own `/open` route. The document goes into a sandboxed frame that
runs no script; `useFrame` sets the sandbox itself, follows the
document once it is parsed, keeps the frame as tall as its content and
hands the app's keys through.

`/open` needs no session. The link rides the fragment, which never
reaches a server. A device gets its key when the mail screen loads and
loses it when the session ends there; a message writes it into every
link. A plain link made on this device leaves for its target at once.
A mail address made on this device goes to the mail program and the
tab stays for the reader to close. A web link shows
where it leads and asks first, with Cancel in focus, when its host is
an internationalized name, its text names another host, it points at
the app itself, its text found no room in the address or this device
did not make it. An address the page cannot read opens nothing and
neither does a mail address this device did not make.

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
account switcher (a menu with every account of the session, each with
its inbox's unread count and the word Expired or Stopped after the
name of a stopped one, then Settings and Sign out) over the mailbox
tree: the six roles in a fixed order,
then the folders with their depth. The tree is one tab stop: the arrow
keys move inside it and Tab leaves it. Mailbox names are the server's
words; the counts are unread mail, and drafts for the drafts folder.
Each mailbox has a jump, `g` then its letter: the six roles keep fixed
letters and a folder takes the first free letter of its name, shown
beside it at the desktop width; a folder without one is reached
through the palette. A jump puts the focus on the list's first row.
Cmd/Ctrl+Shift+L opens the account menu at the tablet and desktop
widths, where the switcher is on the screen.

Under the header sits the thread list, a grid over the pages the cache
serves, rendered only where it is in view, so a mailbox of fifty
thousand messages scrolls like one of fifty. A row is a thread as this
mailbox shows it: sender and time on the first line, subject and
preview on the second, a dot and weight for unread, a count for a
thread of more than one and icons for a flag and an attachment; a
screen reader hears the row in one go. One row carries the tab stop:
the arrow keys, Home and End move it inside the grid, j and k from
anywhere on the screen. New mail never moves the rows: a marker names
it above the list until the dot key or a click brings it in, and a
scroll or a move in the list puts the marker away until more arrives.
Offline, a strip over the list says so while the cached rows stay. A
stopped account shows a banner over its list: an expired connection
with Reconnect to the card, a server that could not be reached with
Retry, whose outcome lands in the same sentence; a pass says so, hands
the focus to the first row and fades the banner. A
mailbox still fetching its headers shows its progress at the foot of
the pane, with still rows after the last synced one; the worker polls
closer while such a sync runs. Otherwise the foot holds a strip of key
hints read from the registry, where a keyboard is likely: at the
tablet and desktop widths. A render failure in one pane stays in
that pane. The add-account card, the settings screens and the page a
link opens each load as a chunk of their own on their first visit; the
thread pane, the
account menu and the command surfaces load as chunks when the shell or
the layout mounts, the still cards or a plain trigger standing in until
they land; the message details load as a chunk when a card opens, so
they open at once.

Enter, `o` or a click opens a row's thread at
`/mail/{accountId}/{mailboxId}/{threadId}`, where the reading pane
preference puts it: beside the list; below it, behind a seam that moves
by rows and keeps its place on the device; or as a screen of its own,
which a phone always uses. The list stays where it was underneath, so
Escape, the Close button or the way back return the focus to the row.
The pane shows the thread's subject, its message count and one card per
message: who wrote it, to whom and when, over its body. The newest
message and the unread ones open, the two before them stay in sight
collapsed with their first sentence and the rest wait behind a button.
The cards in sight and the open ones stay as the pane first drew them
while the thread changes under it. A thread opens as it stands: one
that changed since it was last read waits for the fresh read before
its cards draw. The members' headers are fetched with their previews
when a thread opens, in batches of the size the bridge fills previews
in.

From the repo root: `pnpm build` builds it, `pnpm test` runs the unit
tests and `pnpm test:e2e` runs the Playwright suite. `pnpm dev` inside
this directory starts the dev server and `pnpm storybook` the component
workshop. The frame traces run alone, without the rest of the suite,
with `pnpm exec playwright test --project scroll-budget --no-deps`.
A frame counts the main thread's own CPU time, read from a Chromium
trace: the task that paints it (the scroll event, the render, the
layout and the paint) and the tasks that run before the frame's closing
timer. Time the thread waits for a core does not count. The phone
trace slows the CPU by the host's Lighthouse benchmark index over 1000,
the middle of the high-end mobile bracket in Lighthouse's throttling
doc, so a mid-range phone means the same on every machine. Each trace
prints one line with the index, the factor, the longest frame, the 95th
percentile and any frame past the budget. The `body-budget` project
runs after it: it opens a newsletter of 102 KB, the weight Gmail clips
a message at, on the phone profile and prints the time to the still
lines and the longest task of the main thread while the message
builds. It also opens a mail of twenty thousand remote images, which
has to stand at its height within twenty seconds.

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
