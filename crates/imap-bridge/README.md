# huliho-imap-bridge

Translates JMAP Mail semantics (RFC 8620, RFC 8621) to IMAP4rev1
(RFC 3501) with its extensions and SMTP submission (RFC 6409), so the
rest of Huliho only speaks JMAP.

The first piece is the credential check at add time. `verify` connects
to an IMAP server, signs in with a password or an OAuth token, reads the
capabilities and logs out; `smtp::verify` does the same against the
submission server with EHLO and AUTH, so a mailbox that refuses SMTP
AUTH is caught before anything is stored. Both connections use TLS from
the first byte or upgrade with STARTTLS before any credential is sent;
a server that does not offer STARTTLS is refused, never spoken to in
plaintext. A NO carrying the RFC 5530 `UNAVAILABLE` code means the
server could not judge the credential; it reads as unreachable, not as
a refusal. Certificates are validated against the trust the caller
hands in and nothing else. The caller resolves the host and checks the
addresses; the bridge connects to them in order and resolves nothing
itself. Every step runs within one timeout. No error carries the
credential or the server's own words. The session layer sits behind one
narrow trait, so the client library can be swapped in one module.

Every byte a server sends passes a guard before the client library
parses it. The protocol parser recurses once per open parenthesis,
spends many times the bytes of a response on the heap outside its
literals and the library buffers a response whole. So a response that
opens more than 32 levels (`session::MAX_NESTING`), takes more than
1 MiB (`session::MAX_RESPONSE_BYTES`) or takes more than 64 KiB
outside its literals (`session::MAX_STRUCTURED_BYTES`) fails the
connection with a fixed sentence, before STARTTLS as well as after it.
The guard follows quoted strings and literals only on untagged data
lines, so parentheses in a subject or a file name never count. On a
status line, a tagged line and a continuation it counts every byte.
The size marker of a literal fails the connection there, since some
response codes let the parser take one and the two could then disagree
on where a response ends. The bridge reads the answers to its own
commands one response at a time. CAPABILITY is one of them and travels
under a tag of the bridge's own, since the client library would hold
every line nobody asked for until the tagged one.

The read path begins with the mailbox list. `mailboxes::sync` runs
LIST with the RETURN options the server's capabilities allow
(SUBSCRIBED, SPECIAL-USE and STATUS behind LIST-EXTENDED, SPECIAL-USE
and LIST-STATUS). What the listing does not carry it asks separately:
LSUB for the subscriptions, one STATUS per selectable mailbox for the
counts. It maps the lines to rows: roles from the attributes first and
from a fixed table of names second, one mailbox per role, names decoded
from modified UTF-7, the hierarchy from the delimiter, the counts from
STATUS. A mailbox without a STATUS answer, because the server said NO
or left its line out of the listing, keeps its row with zero counts and
the pass goes on. A listing of more than ten thousand mailboxes fails
the pass and nothing is written; a name that holds a control character
or passes 4096 bytes cannot be sent back on a command line and is left
out. The bridge speaks IMAP4rev1 with extensions and
never enables IMAP4rev2 or UTF8=ACCEPT, so servers keep modified
UTF-7 names toward it; a server that dropped IMAP4rev1 is refused.
Every read command runs on the client library's raw command path and
the bridge reads each untagged line itself, so a STATUS line after a
LIST is kept.

The header sync fills the rows. `sync::FolderSync` opens a folder with
EXAMINE and lists its UIDs with one UID SEARCH per five thousand
sequence numbers, from the top down, so no answer grows with the folder.
It fetches them newest first in batches of five hundred
(`sync::SYNC_BATCH`): the flags, INTERNALDATE, the size, BODYSTRUCTURE
and eleven header fields, never a body. One batch is one fetch, one
transaction and one state. The progress is part of it, so a restart
carries on below the last UID written and `syncedEmails` grows while
`totalEmails` stands. Flags become keywords (RFC 8621 section 4.1.1), a
message flagged `\Deleted` never arrives and `hasAttachment` follows the
structure unless Dovecot's two flags decide. Once a folder is done its
email counts come from the rows instead of STATUS. The header bytes are
whatever a sender wrote: at most 64 KiB per message are kept
(`session::MAX_HEADER_BYTES`), mail-parser decodes them and the
addresses, the subject and the message ids go into one blob the host
seals. A fetch that fails on what the server sent is halved until the
one message behind it stands alone; that message is fetched once more
without its structure and left out when that fails too, so no message
the server describes badly can stall a folder. A fetch the server
answers with NO goes back whole, since a NO may pass; the host bounds
those retries. The sync outlives a session: after a failure the host
connects again and calls `resume`. A folder the server renumbered keeps
the ids of the messages it still holds, matched by the hash of the
Message-ID, INTERNALDATE and size, up to a hundred thousand rows
(`store::REMATCH_LIMIT`); a mailbox that vanished takes its emails along
in the state of the pass.

Threads are computed as the rows arrive. `bridge_message_ids` is a
union-find over the keyed hashes of the message ids a header names
(Message-ID, In-Reply-To and References, the REFERENCES algorithm of
RFC 5256 without its subject step): every id points at one thread, a
message joins the thread its ids name and a message that names two
threads merges them. The thread that holds more emails survives, the
smaller id on a tie, so a merge moves the fewer rows; the thread that
loses is destroyed and its emails read as updated. A thread leaves with
its last email. `Thread/get` answers the emails of a thread by
`receivedAt`, oldest first, ties by id.

A Gmail account keeps its mail in three folders and shows it under
labels. The host names the account a Gmail account
(`sync::Cache::gmail`) and the server confirms it with the `X-GM-EXT-1`
capability; without the capability the account runs as folders. With
it, the `\All`, `\Junk` and `\Trash` folders are the stores the sync
reads (All Mail with the archive role) and every other selectable
folder is a label mailbox without rows of its own; the `gmail` module
holds those rules and `mailboxes::mapping` writes them into the columns
`store` and `gmail_label`. The header fetch on such an account asks
`X-GM-LABELS`, `X-GM-MSGID` and `X-GM-THRID` next to its items and both
flag fetches ask `X-GM-LABELS` next to the flags; the first 64 labels
of a message that can go on a command line are kept and an item the
command did not ask for is dropped. A row of All Mail is a
member of All Mail and of every label mailbox its labels name; a row of
Spam or Trash is a member of that store alone. INBOX and the folders
with the `\Sent`, `\Drafts`, `\Flagged` and `\Important` attributes
show their system labels; a user label shows in the folder of its name;
`\Draft` and `\Starred` read as `\Drafts` and `\Flagged`. One message
is one row, told by X-GM-MSGID across the stores: a message that turns
up in another store moves its row there and keeps its id and its
thread; a second live UID of one folder under the same id is left out.
A row whose UID left a store waits for the pass with its UID negated
and leaves at the end of the pass when no store claims it, so a move
keeps its id whichever store the pass reaches first. Threads are the
server's: `t` followed by the X-GM-THRID in decimal; no hash enters
`bridge_message_ids`. A message whose line lacks one of the three items
is stored as a folder account stores every message. A label mailbox's
counts come from STATUS until All Mail is done and from the memberships
afterwards; the refresh visits the stores alone and a vanished label
mailbox drops its memberships. The session object says
`maxMailboxesPerEmail` is null on such an account.
`runtime::GMAIL_CONNECTIONS` names the two connections one Gmail
account may hold at once, which the host's runtime enforces.

The connection comes from the host through the `runtime::Connector`
trait: `connect(key)` answers a signed-in session or a typed failure,
so the bridge never sees a credential and never resolves a host.
`runtime::Link` keeps one conversation per account between requests
behind a lock, checks a kept session with NOOP before it is used again
and drops it after a failure. Only a refresh, a preview fetch, a body
fetch and a window of a blob take the conversation; a request that
reads the cache never waits on IMAP, and a server that is down costs
nothing but the refresh for the stored properties: the cache answers as
it stands, while a body ask answers `serverUnavailable`.

`runtime::Bridge` runs the accounts the host registers: one store, one
sealer and one connector for all of them and, per account, the
conversation requests take and a task that syncs every store folder
whose first sync is not done, in tree order. The task takes one batch
per turn on the conversation and hands it back, so a request gets in
between, selects the folder afresh before every batch and puts one
deadline (`runtime::CONVERSATION_DEADLINE`, sixty seconds) around a
batch, a refresh, a preview fetch, a body fetch and a window of a
blob. A session that fails or runs past
the deadline is dropped; a fresh one resumes the same sync after a
pause, and the slice whose fetch was cut short is fetched again. A
folder whose failures pass `runtime::FOLDER_FAILURE_BOUND` waits for the
next round. Two accounts sync at once process wide
(`runtime::SYNC_PARALLELISM`); a Gmail account holds a second
conversation for its sync. A conversation nobody used for five minutes
(`runtime::SESSION_IDLE_CLOSE`) is logged out and opened again on the
next request. The host hands every account a `runtime::Registration`:
its key, whether it is a Gmail account and the state its session object
shows, which the host derives from what the object is built from, so
the state never moves with the cache. `forget` stops an account's task,
breaks off a blob still on its way and refuses every later write for
its key, so the host deletes the rows (`store::remove_rows`) inside the
transaction that removes the account.

Change detection is on demand until the server pushes. A `/changes`
call refreshes the account at most once per thirty seconds
(`runtime::REFRESH_INTERVAL`): one mailbox pass, then per folder whose
first sync is done the new mail above the recorded UIDNEXT, fetched
oldest first in the batches and under the narrowing of the first sync,
the flags that changed and the messages that left. Where the server
advertises CONDSTORE the flags come as one
`UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE n)` from the mod-sequence the
folder stood at; a folder whose answer passes ten thousand messages
(`session::MAX_FLAGGED`) takes a scan of its stored UIDs range by range
from then on, which is also what the mailbox the client looked at last
gets on a server without CONDSTORE. A stored message that gained
`\Deleted` leaves. An expunge shows in the count: when MESSAGES falls
short of the count on record plus the messages the refresh fetched, the
UID list comes in the windows of the first sync and the stored UIDs it
lacks leave. A folder whose UIDNEXT, HIGHESTMODSEQ and MESSAGES stand
as the cache last saw them costs no command; the one exception is the
mailbox the client looks at on a server without CONDSTORE, since a flag
change shows in no STATUS item there. The row of a folder in
`bridge_sync` says how far it is
synced (UIDNEXT, HIGHESTMODSEQ and MESSAGES as the server had them),
moved inside the transaction that writes the work it stands for, so a
refresh cut short by a lost connection claims nothing it did not write
and the next one carries the walk on. Every write is one state in
transactions of at most a batch.

`preview` is fetched on demand. The part it is read from is chosen when
the header sync reads the structure: the first `text/plain` leaf, else
the first `text/html` leaf and never an attachment. It travels in the
sealed blob with its size. An `Email/get` that asks for previews the
rows lack fetches them, one hundred at most (`jmap::PREVIEW_BATCH`), in
one `UID FETCH` per folder, part and length the messages share, every
item a partial fetch (RFC 3501 section 6.4.5): the MIME header of the
part to 4 KiB (`session::PREVIEW_HEADER_BYTES`), a plain part to 2 KiB
(`sync::preview::PREVIEW_PLAIN_FETCH_BYTES`), an HTML part under 64 KiB
(`PREVIEW_HTML_PART_BYTES`) to that bound and a larger one to 16 KiB
(`PREVIEW_HTML_FETCH_BYTES`), so what a sender wrote arrives inside
literals the ask bounds and a size a server claimed is never trusted.
mail-parser decodes the pair as a message, HTML becomes text and the
first 256 characters (`PREVIEW_CHARS`) are kept in the blob as one
state, each email logged as updated. A message stored without its
structure serves an empty preview and asks for nothing.

A body comes from the server when a message is opened; nothing of it
is stored. An `Email/get` that asks a body property or a `header:`
form runs twice: the first pass notes which messages it needs, the
request fetches them on the account's conversation under the deadline
and the second pass answers, every call after it run again so a
reference into its answer resolves; a call that asks only then gets a
third pass and no more. Per message one `UID FETCH` brings
BODYSTRUCTURE and the header fields named, the fields as a partial
fetch of 64 KiB. The tree becomes the EmailBodyPart tree of RFC 8621
section 4.1.4 with the part number as `partId`, the three lists follow
the walk that section suggests and a part's `blobId` is the email id,
a hyphen and its number with underscores for dots; a message inside a
message is a leaf. The text parts the fetch flags name come in windows
of 512 KiB (`session::BODY_WINDOW_BYTES`) up to the client's
`maxBodyValueBytes` and never past 8 MiB (`jmap::MAX_BODY_VALUE_BYTES`),
each window one literal under the byte bound. Once the conversation is
given back, the MIME header written from the structure and the windows
decode through mail-parser off the runtime, line endings fold to LF
and a value cut at its cap or short of its part is marked truncated.
One request fetches 32 windows at most, 16 MiB on the wire, what the
proxy admits of a native server's answer
(`jmap::MAX_BODY_WINDOWS`); a value the budget does not reach is empty
and marked truncated. A call reads the server for four messages at
most (`jmap::MAX_BODIES_IN_GET`) whatever it asks of them and names 32
header fields at most (`jmap::MAX_HEADER_FIELDS`); a tree keeps 64 KiB
of fields at most (`session::MAX_TREE_BYTES`), since a literal in a
BODYSTRUCTURE lies outside the guard's structure bound. A message the server cannot describe
within the bounds answers one `application/octet-stream` part over the
whole message, named `message.eml` and listed as its one attachment;
the messages after it are read on a fresh session, two such sessions
per pass of a request at most, shared by every body ask of the pass,
the messages left after the second absent. A message the server does
not hold is `notFound`; a server out of
reach, a NO on a message's own fetch and a window answer the bridge
cannot read answer `serverUnavailable` for the call. The `header:` forms serve
Raw and Text, each with `:all`; the Text form is refused for the
structured fields of RFC 5322 and RFC 2369.

A blob (RFC 8620 section 6.2) is the whole message under its email id
or one part under the `blobId` its body part shows. `blob::open` reads
one for the pair of a cache and a link; `Bridge::blob` runs it for an
account. The id is computed and never stored: one that names no message
of the account, no part of its message or a multipart is not found. The
bytes come in the windows of a body value, each on a turn of its own on
the conversation: the folder examined afresh, then one
`BODY.PEEK[<section>]<offset.length>` with the empty section for the
whole message. The conversation is free between two windows. A message
streams as it lies on the server. A part streams with its transfer
encoding undone through mail-parser's decoders: a base64 window is cut
at its last whole quartet and a quoted-printable one at its last line
break, what is left waits for the next window, so the edge of a window
changes nothing in what a part decodes to. Bytes no encoder writes
never fail a part: a byte outside the base64 alphabet is ignored (RFC
2045 section 6.8), an `=` of quoted-printable that opens neither an
escape nor a soft break is read as itself and every line break decodes
to CRLF (section 6.7); a quoted-printable line past 1000 bytes is cut
where it stands. Any other encoding
leaves the bytes as they are. A blob ends at the first window the
server cuts short. `open` reads the first window itself, so a blob that
is not there or cannot be read fails before a byte is handed on. A task
of its own reads the windows after it ahead of the reader, four at most
(`blob::BLOB_BUFFER_WINDOWS`); it waits while they are unread and ends
when the reader leaves, so a slow reader holds those windows beside the
first one and never the conversation. A stream that stops without its
closing step answers an error, so bytes cut short never read as a whole
blob. The caller names a limit in octets.
A message whose row states more and a part whose structure states more
are refused before a window is read; a blob the server sends more of
breaks off. A NO on a fetch and a folder whose UIDVALIDITY moved leave
the session standing and the blob unavailable; any other failure costs
the session as well. An error is the last thing a blob answers. The
bridge hands bytes: the host decides their media type and disposition.

The rows live in `bridge_` tables inside the host's database.
`store::MIGRATIONS` carries their schema for the host's migration list
as two migrations, the tables and then the index on the message ids by
thread with the progress columns of the refresh; every row carries the
host's opaque account key. Ids are a type letter in front of UUID text.
The state string is a per-account counter; every change writes a row
to a log that the three `/changes` methods read, kept for the newest
ten thousand rows. `jmap::handle` runs a Request object (RFC 8620
section 3.3) against those rows and answers `Mailbox/get`,
`Mailbox/changes`, `Email/get`, `Email/query`, `Email/changes`,
`Thread/get`, `Thread/changes` and `Core/echo`. `Email/get` serves the
metadata, the header properties and the preview from the rows, the
body properties and the `header:` forms from the server. `Email/query` serves the filter
`inMailbox`, the sort `receivedAt` either way, a window by position or
by anchor, the total on request and one email per thread on request,
two hundred ids at most (`jmap::QUERY_LIMIT`); any other filter or sort
answers `unsupportedFilter` or `unsupportedSort`. Result references
resolve under a budget of one MiB per request, measured as the memory
a copy of the resolved values takes; a reference past it answers
`invalidResultReference`. `jmap::session_object` renders the session
object with the core and mail capabilities and the vendor capability
`https://huliho.com/jmap`, which carries the Mailbox property
`syncedEmails` and is followed only when a request names it. The host
supplies the URLs. `Sealer` is the trait behind which the host keeps
its cryptography for the personal fields of an email row; the sync and
`jmap::handle` reach it through `sync::Cache`.

Other software reuses the bridge by running it as its own process and
speaking JMAP to it. Linking the crate into a program instead creates a
combined work, which must ship under the AGPL (see LICENSE and NOTICE).

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`. `cargo test -p huliho-imap-bridge --features
live-targets` adds the tests that need the compose Dovecot.

The `test-support` feature exposes `testing`, the scripted IMAP and
SMTP servers the tests run against: a fresh certificate per server, one
answer per command and a record of every line received. Its IMAP script
carries a mailbox model with LIST-EXTENDED, LIST-STATUS, SPECIAL-USE
and CONDSTORE as switches and a message model behind EXAMINE, NOOP,
UID SEARCH and UID FETCH, the flags alone with CHANGEDSINCE, the two
sections of a preview cut as the partial fetch asks, the structure with
a set of header fields, the fields alone and one window of one section
or of the whole message
included, whose misbehavior is a switch as well: volunteered lines, a connection that
drops, a MODSEQ item, a NO in raw UTF-8, messages that leave between
two searches, the Gmail items volunteered unasked plus an EXAMINE answer
without EXISTS. Its `Mailboxes`
stand in for a second client between two passes: a message appended,
one expunged by UID, the flags of one replaced, the counts and the
mod-sequences following. Its Gmail mode
(`Mailboxes::gmail`) lists the folders as Gmail does, answers the three
Gmail items where asked and BAD where the extension is off, counts a
label folder from the labels of All Mail's messages and lets a test
relabel a message or move it between stores as a second client would.
`testing::TestConnector` signs a
user in on such a server or refuses every connection.
`testing::seal::TestSealer` binds a blob to its row without a cipher.
`Blob::scripted` builds a blob from a channel, for a host that tests
its own reader of one.
`session::fuzzing`, `jmap::fuzzing` and `blob::fuzzing` are what the
fuzz targets in `fuzz/` call.
Build them with
`cargo build --manifest-path crates/imap-bridge/fuzz/Cargo.toml` and run
one with cargo-fuzz on a nightly toolchain.

The transcript suite replays what a provider said. `testing::record` is
a server on a loopback port that hands one conversation on to a live
server and writes both sides down. When the recording ends it replaces
every address, subject, message id and Gmail id with a fixed value, the
same one wherever it recurs, turns every body text into filler of its
length and every greeting into `ready`, then scans the result and hands
it over only when nothing of the live session remains; a value that
already has a fixed shape stays as it is. What the pass does not name,
a file name in a BODYSTRUCTURE or a label name for one, stays as the
server sent it, which is why a fresh recording is read whole before it
is checked in. `testing::replay` is the
scripted server driven by such a transcript: every command gets what the
live server answered, under the client's own tag, the connection closes
where the live one closed and a command the transcript does not hold is
a mismatch the test reads. Gmail's transcript is
`tests/transcripts/gmail.json`, replayed by the scenario suite in
`tests/gmail_transcript.rs` in every test run; the same suite runs
against the scripted server's Gmail mode through the recorder and back
through the replayer, and against a live test account when
`HULIHO_LIVE_GMAIL_ADDRESS` and `HULIHO_LIVE_GMAIL_APP_PASSWORD` are
set, writing the transcript to the path in `HULIHO_RECORD_TRANSCRIPT`
when that is set as well. `tests/transcripts.rs` scans every checked-in
transcript.
