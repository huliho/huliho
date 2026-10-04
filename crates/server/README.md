# huliho-server

The Huliho server binary. One process serves the built web app and the
API: static assets with an SPA fallback, `/healthz` for liveness, the
security headers on every response, request-scoped tracing and a
graceful shutdown on SIGTERM.

All persistent state lives in an embedded database inside one data
directory, created on first start and migrated on every start. The
schema covers organizations, users with fixed roles and a display
name, connected accounts, server-side sessions with a device record
and the address of their last use, an append-only domain event log
with configurable retention, per-user preferences and per-sender
policies. Every read and mutation requires a typed scope from the
single resolver, so nothing reaches storage across an organization or
user boundary. A signed-in user lists their own sessions and ends any
of them but the current one; every mutation stamps its session at most
once per five minutes and records the user as active for the month.
A user changes their own password against the current one; every
other session ends and the current one continues on a fresh token.
An admin creates users and resets passwords through a one-time
password that is shown once, works for one sign-in within a day and
opens a session that reaches only the password change. A user's
connected accounts are rows carrying the address, a display name, the
provider preset and the connection settings; the credential sits beside
them sealed under a key of its own and bound to the row, so it never
reaches the browser. A signed-in user lists their own accounts and
removes any of them; the credential leaves with the row.
Adding an account starts with discovery: the server takes a mail
address and answers the server behind it. The well-known mail domains
name their preset without a lookup; every other domain runs a chain of
the JMAP well-known resource and its SRV record, the RFC 6186 SRV
records, the Thunderbird autoconfig documents and the MX names of the
known providers, in that order, each step within five seconds and all
of them within fifteen. Every outbound connection resolves through one
resolver that refuses private networks unless the config lists them,
follows at most three redirects over HTTPS to named hosts only and
validates certificates against the built-in roots plus the configured
CA file. Discovery counts against the sign-in rate limiter.
Connecting the account is the second step. The client sends the
address, the target it confirmed and the credential once; the server
checks the credential upstream before it stores anything. A JMAP
account is checked against its session resource with Basic or Bearer
authentication and must advertise the mail capability. An IMAP account
is checked through the bridge, the IMAP sign-in first and then the SMTP
submission sign-in with the same credential, so a mailbox that refuses
SMTP AUTH is caught at add time. Every target, discovered or typed, is
validated the same way, resolved once through the pinned resolver and
connected with the instance's TLS trust; each connection gets twenty
seconds. Connect counts against the sign-in rate limiter like
discovery. The answer is the account row or a stable error code naming
the cause: a refused credential, an unreachable server, an insecure
connection, a server that is not usable or a submission server without
SMTP AUTH.
Google and Microsoft accounts sign in through a consent instead of a
password. The instance holds one OAuth client per provider, registered
over the API by an instance admin with the secret sealed like an
account credential; the flag comes from `huliho instance-admin grant
<login>` on the operator CLI and `revoke` takes it back. With a client
and the public URL set, the app starts a consent and gets the
provider's URL to open in a window: code flow with PKCE, a state bound
to the user, the least scopes a mail client needs. The session answer
lists the providers a consent can start with, so the app shows a
sign-in button only where it works. The provider sends
the window back to `/auth/{provider}/callback`, where the code turns
into tokens through the same pinned HTTP client as every other outbound
request, the tokens are checked with XOAUTH2 on IMAP and SMTP and
sealed on a new row or on the row being reconnected. The window then
shows one sentence in the browser's language; the app polls the
outcome. A consent lives ten minutes and is claimed once; its owner
can end it before that through `DELETE
/api/accounts/oauth/pending/{state}`, after which the callback lands
nothing and any other state answers 404. An access
token about to run out is refreshed before use and the rotated tokens
are written back; a provider that refuses the grant stops the account.
Request logs carry the matched route, never a path with an id or a
query in it.
Once an account is connected, every attempt on it passes through one
gate. A rejected credential stops the account at once and the row says
so with the cause `credentials`; refused, timed-out or TLS-failed
connections in five separate ten-second windows stop it with the cause
`connection`; failures inside one window count once. The stop
lives on the row, so a restart does not resume retrying by itself. A
probe checks every account stopped on connection at startup and then
at the configured interval and resumes it when the server answers
again. The user retries a stopped account with
`POST /api/accounts/{id}/retry`, which answers the row or
`still_stopped` with the cause. A new credential goes to
`PUT /api/accounts/{id}/credentials` and replaces the stored one after
a passing check. An OAuth account whose access token is about to run
out gets a fresh one before the check. Checks on one account run one
at a time.
Mail itself moves through one JMAP endpoint per account. `GET
/api/jmap/{id}/session` answers the account's session object as the
browser may see it: its URLs point at the proxy, its capabilities are
cut to core and mail and its two request limits are at most the
proxy's own.
`POST /api/jmap/{id}` forwards a Request object to the API endpoint
that session object named, with the account's credential added on the
way and the answer handed back as the server sent it, with one
exception: an `Email/get` that asks for body values comes back with
every `text/html` value its `htmlBody` names sanitized, for a native
account and a bridge account alike, so no sender's HTML reaches the
browser unsanitized. Such a request is refused before anything connects
when its `properties` list lacks `htmlBody`, when it sets
`fetchAllBodyValues` or when a result reference stands in for one of
those arguments; an `Email/parse` that would carry body values is
refused the same way. The sanitizer keeps structure, text, lists,
tables, inline marks, images and style blocks; it drops scripts, event
handlers, forms, embedded documents, head content and foreign
namespaces, sets every link to open in a new tab without an opener and
admits `http`, `https` and `mailto` on a link and `http`, `https`,
`cid` and a `data:image/` URL on an image, never a relative URL, a URL
on the instance's own host or a path under `/api/`. A request body
of one MiB at most, an answer of sixteen MiB at most, one timeout per
upstream request and four requests in flight per account; a fifth one
gets the JMAP `limit` error at once. The API endpoint an upstream
names passes the pinned resolver and the private-network rule like
every other target before anything is sent to it. Every outcome
reports to the gate without the account lock: a rejected credential
stops the account at once; refused connections count once per
ten-second window and stop it after five windows; a server error of
the upstream's own decides nothing; a stopped account answers
`still_stopped` before anything connects.
`GET /api/jmap/{id}/download/{accountId}/{blobId}/{name}?type=` is
the route the rewritten `downloadUrl` names. For a native account it
expands the template the upstream session object named with those
four values, checks the result like the API endpoint and streams the
upstream's answer with the account's credential added. The type of
the answer comes from the blob's first bytes: one of the six raster
types a browser renders, asked as that type, answers inline with that
type; every other blob, an SVG or a text file included, answers
`application/octet-stream` as an attachment, whatever the sender or
the request said. Every blob answer carries `nosniff`, a policy that
renders and fetches nothing, `Cross-Origin-Resource-Policy:
same-origin`, `Referrer-Policy: no-referrer` and a day of private
caching, or `no-store` under `[privacy] strict`. A blob whose declared
size passes 64 MiB is refused before its first byte and an undeclared
one that runs past it breaks off; two downloads stream at once per
account beside the request cap; a third waits for a lane for twenty
seconds before it answers 429; an upstream gets twenty seconds to
answer with its headers and twenty seconds per chunk after that, and
one download gets ten minutes in all; a `Range` from byte zero answers
206 with exactly those bytes when the blob's declared length runs past
the range; a range that reaches the end of the blob, an undeclared
length or any other range answers the whole blob with 200. For an IMAP
account the bridge reads the blob from the server in windows: the whole
message under its email id, a part under its blob id with the transfer
encoding undone. Such a blob declares no length, since an IMAP server
states the size of a message as it counts it, which may be an estimate,
while the size of a part is that of its transfer encoding: the answer
carries no `Content-Length` and a range answers the whole blob. A blob
whose stated size passes 64 MiB is refused before a window is read. An
id that names no blob of the account answers `not_found`, as does any
account id on the path but the account's own. A server the bridge
cannot read answers `upstream_failed`; one that fails mid-stream breaks
the download off. The same lanes hold. The ten minutes start before the
bridge is asked, so the wait for the account's conversation counts; the
first window answers within the bridge's own deadline for a turn of
sixty seconds and every chunk after it within the twenty seconds of
the route.
`GET /api/remote-image?url=` fetches an image a message links, on the
reader's behalf and with nothing of theirs on the request. The URL
stays within 2048 bytes, carries no user information and names a host
rather than an address; an `http` URL is fetched over `https` on the
same host. That host and the target of every redirect after it, three
at most, pass the pinned resolver and the private-network rule before
anything connects. A URL off the other rules answers `invalid_request`
without a lookup; a host inside a private network answers it after one
lookup and no connect. The request carries a fixed `User-Agent`
and no cookie, authorization or referrer; each attempt gets ten
seconds. The bytes are read up to 5 MiB and refused past it with
`too_large`. They must carry the signature of one of the six raster
types and the answer names that type; bytes without one answer
`not_an_image` and a host that answers anything else
`upstream_unreachable`. The image comes back with the blob headers
above. A session may fetch 400 images at once and 120 a minute after
that; past that the route answers 429. Thirty-two images, each read
within 5 MiB, are fetched and buffered at once per process; a fetch
past that waits ten seconds for a slot before it answers 429.
`GET /api/preferences` and `PUT /api/preferences/{key}` read and write
the reading pane position, the theme, the density, the locale, the font
size, the line height and the dark-mode treatment of messages, each from
a fixed list of words. `GET /api/sender-policies` lists the signed-in
user's per-sender policies, up to five thousand rows; `PUT
/api/sender-policies/{sender}` writes one, the sender lowercased and at
most 320 bytes, the key `remoteContent` with a value `{ "allow": true,
"authserv": <id or null> }`, the id empty for a header that named no
server; `DELETE
/api/sender-policies/{sender}/{key}` removes one. `GET /api/session`
carries `privacyStrict`, the instance's `[privacy] strict` setting.

Configuration is one TOML file; unknown keys are rejected. Top-level
keys are the `listen` address, the `assets` directory and the optional
`public_url`, the base URL the instance is reached on, which the
provider sign-in needs for its redirect URI and the sanitizer reads as
the instance's own host, so no mail can name an image or a link there.
`[storage]`
holds the data directory `path` (default `data`) and `[events]` holds
the event log `retention_days` (default 365). `[auth]` holds the
`secret_file` path plus the session `idle_timeout_minutes` and
`absolute_timeout_minutes`. `[upstream]` holds the rules for reaching
mail servers. `allow_private_networks` lists the private networks an
upstream may resolve to, written as CIDRs. It is empty by default.
`additional_ca_file` names one PEM bundle trusted next to the built-in
roots. `probe_interval_minutes` says how often a stopped account is
checked for recovery, fifteen by default. Discovery and the credential
check read the network rules and the CA file; the account list reports
the probe interval and the probe runs at it. `[privacy]` holds
`strict` (default `false`): the client then keeps its cache in memory
only and blob answers carry `no-store`. The file path comes from
`HULIHO_CONFIG` and
that file must exist. Without the variable the server reads
`huliho.toml` from the working directory and falls back to the defaults
when it is absent.

The binary is `huliho`; without a subcommand it serves. Build and test
from the workspace root: `cargo build` and `cargo test --workspace`.
`cargo test -p huliho-server --features live-targets` adds the tests
that need the compose targets and the public internet. The XSS corpus
under `tests/xss/` runs on every test run; `cargo build --manifest-path
crates/server/fuzz/Cargo.toml` builds the fuzz targets for the sanitizer
and the download template, run with cargo-fuzz on a nightly toolchain.
