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

The rows live in `bridge_` tables inside the host's database.
`store::MIGRATIONS` carries their schema for the host's migration list
and every row carries the host's opaque account key. Ids are a type
letter in front of UUID text. The state string is a per-account
counter; every change writes a row to a log that `Mailbox/changes`
reads, kept for the newest ten thousand rows. `jmap::handle` runs a
Request object (RFC 8620 section 3.3) against those rows and answers
`Mailbox/get`, `Mailbox/changes`, `Email/get` and `Core/echo`.
`Email/get` serves the metadata and the header properties; a body
property is an unknown one until bodies arrive. Result references
resolve under a budget of one MiB per request, measured as the memory
a copy of the resolved values takes; a reference past it answers
`invalidResultReference`. `jmap::session_object` renders the session
object with the core and mail capabilities and the vendor capability
`https://huliho.com/jmap`, which carries the Mailbox property
`syncedEmails` and is followed only when a request names it. The host
supplies the connection and the URLs. `Sealer` is the trait behind
which the host keeps its cryptography for the personal fields of an
email row; the sync and `jmap::handle` take it as an argument.

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
and CONDSTORE as switches and a message model behind EXAMINE, UID
SEARCH and UID FETCH whose misbehavior is a switch as well: volunteered
lines, a connection that drops, a MODSEQ item, a NO in raw UTF-8,
messages that leave between two searches plus an EXAMINE answer without
EXISTS. `testing::seal::TestSealer` binds a blob to its row without a
cipher. `session::fuzzing` is what the fuzz targets in `fuzz/` call.
Build them with
`cargo build --manifest-path crates/imap-bridge/fuzz/Cargo.toml` and run
one with cargo-fuzz on a nightly toolchain.
