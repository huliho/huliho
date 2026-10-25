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
or passes 4096 bytes (`session::MAX_WIRE_NAME_BYTES`) cannot be sent
back on a command line and is left out.
The bridge speaks IMAP4rev1 with extensions and
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
`runtime::GMAIL_CONNECTIONS` names the three connections one Gmail
account may hold at once, which the host's runtime enforces.

The connection comes from the host through the `runtime::Connector`
trait: `connect(key)` answers a signed-in session or a typed failure,
so the bridge never sees a credential and never resolves a host.
`runtime::Link` keeps one conversation per account between requests
behind a lock, checks a kept session with NOOP before it is used again
and drops it after a failure. Only a refresh, a preview fetch, a body
fetch, a write and a window of a blob take the conversation; a
request that reads the cache never waits on IMAP, and a server that is
down costs nothing but the refresh for the stored properties: the cache
answers as it stands, while a body ask and a write answer
`serverUnavailable`. A search runs on a connection of its own, which
the link keeps beside the conversation.

`runtime::Bridge` runs the accounts the host registers: one store, one
sealer and one connector for all of them and, per account, the
conversation requests take and a task that syncs every store folder
whose first sync is not done, in tree order. The task takes one batch
per turn on the conversation and hands it back, so a request gets in
between, selects the folder afresh before every batch and puts one
deadline (`runtime::CONVERSATION_DEADLINE`, sixty seconds) around a
batch, a refresh, a preview fetch, a body fetch, a write and a
window of a blob. A session that fails or runs past
the deadline is dropped; a fresh one resumes the same sync after a
pause, and the slice whose fetch was cut short is fetched again. A
folder whose failures pass `runtime::FOLDER_FAILURE_BOUND` waits for the
next round. Two accounts sync at once process wide
(`runtime::SYNC_PARALLELISM`); a Gmail account holds a second
conversation for its sync. A conversation or a search connection
nobody used for five minutes (`runtime::SESSION_IDLE_CLOSE`) is logged
out and opened again on the next request. The host hands every account
a `runtime::Registration`: its key, whether it is a Gmail account and
the state its session object shows, which the host derives from what
the object is built from, so the state never moves with the cache; the
bridge marks it while the server advertises QUOTA. `forget` stops an
account's task, breaks off a blob still on its way and refuses every
later write for its key, so the host deletes the rows
(`store::remove_rows`) inside the transaction that removes the account.

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

`Email/set` (RFC 8621 section 4.6) writes keywords and mailboxes and
destroys emails. An update patches `keywords/<keyword>` and
`mailboxIds/<id>` with true to set an entry and null to take it off, or
names the whole `keywords` or `mailboxIds` object. A create answers
`forbidden` per object, an update of an email the same call destroys
`willDestroy`, any other property and any other value
`invalidProperties` with the paths and a call over more than five
hundred objects `requestTooLarge` (`jmap::MAX_OBJECTS_IN_SET`).
`ifInState` is checked before anything is sent. A keyword is stored as
its flag (RFC 8621 section 4.1.1): the four system keywords as `\Seen`,
`\Flagged`, `\Answered` and `\Draft`, any other as an atom. A mailbox
the account does not hold or that holds no mail answers
`invalidProperties`.

On a folder account a message lies in one folder, so a change of its
mailboxes is a move (RFC 8621 section 1.3.1): an update that leaves the
email in no mailbox answers `invalidProperties` with the paths that
took it out and one that leaves it in two answers `tooManyMailboxes`
(RFC 8621 section 4.6). A move is
`UID MOVE` where the server offers it (RFC 6851), else `UID COPY` and
an expunge of what it copied, which needs a folder that keeps
`\Deleted`. The new UIDs come from `COPYUID` (RFC 4315 section 3), so
the row moves to its new folder and UID with its email id kept. A
message the answer pairs with no new UID is `notFound` and its row
leaves; the next sync brings the message where it lies. A server
without UIDPLUS names no new UIDs: the update stands and the rows of
what moved leave the same way. A copy whose expunge fails leaves the
copies beside the messages; the next sync reads them in. A server
with neither MOVE nor UIDPLUS gets no move asked: `mayAddItems` and
`mayRemoveItems` are false on every mailbox and a patch that moves
answers `forbidden`. The mailbox pass reads MOVE and UIDPLUS from the
session's capabilities and logs every mailbox as updated when it first
learns them and whenever either changes. A destroy stores `\Deleted` on
the message and expunges it alone with `UID EXPUNGE`, so a message
another client marked stays; it needs UIDPLUS and a folder that keeps
`\Deleted` and answers `forbidden` without them.

On a Gmail account a label mailbox is a label: an update that adds or
removes one stores `X-GM-LABELS` on the store the row lies in, with
`\Flagged` and `\Drafts` written as `\Starred` and `\Draft`. Entering
Spam or Trash is a move to that store, after which the message carries
that store alone. Leaving either is a move into All Mail: the labels
the patch adds are stored there and the labels the message carries are
read back, since Gmail gives a message back the labels it had; when
All Mail then opens `[READ-ONLY]` or under another UIDVALIDITY than
the move named, the update answers `serverFail` with the row moved
and no label stored. A move between the stores is `UID
MOVE` alone: on a Gmail server that does not offer MOVE it answers
`forbidden` before anything is sent, since a copy and an expunge would
run in All Mail. The three stores show `mayAddItems` and
`mayRemoveItems` only where the server offers MOVE; a label mailbox
keeps a folder's rights. A row whose message the move cannot follow, because
`COPYUID` pairs it with no new UID or the store cannot take it where
it went, waits for the pass with its UID negated instead of leaving;
the pass claims it by X-GM-MSGID where the message turned up and the
update answers as it would on a folder account. A message of Spam or
Trash carries no label, so an update that gives it one without taking
it out answers `invalidProperties`, as does one that enters two
stores, takes a message of All Mail out of All Mail without entering
Spam or Trash or takes one out of Spam or Trash with no label to land
on. A destroy stays in Spam and Trash and answers `forbidden`
anywhere else, since the account's own setting decides what an expunge
does there. A starred message of All Mail is a member of the
`\Flagged` label mailbox: setting `$flagged` makes the email a member
and adding it to that mailbox sets `$flagged`, since Gmail keeps the
flag and the label together.

The write runs between two passes over the request, on the account's
conversation under its lock and deadline: per store folder a SELECT,
then the keyword STOREs, the label STOREs, the moves and the destroys,
each over sets of a hundred UIDs at most (`session::MAX_STORE_UIDS`);
the labels of what moved into All Mail come last. A keyword STORE
carries `+FLAGS.SILENT` or `-FLAGS.SILENT` per set of messages that
gain or lose the same flags, a label STORE `+X-GM-LABELS.SILENT` or
`-X-GM-LABELS.SILENT` the same way; a list past one command line goes
out in runs that fit, and a label too wide for any run fails its
update with `serverFail` before anything is sent. The lock is held
from the state check until the rows are written, so two requests that
name one state never both write. The writes of one round share the
deadline: once it is over, a later write of that round sends nothing
and answers `serverUnavailable`. The calls of a request answer in
order (RFC 8620 section 3.10): a write behind a call that is no write
of the same round waits for the pass after it, and a write the three
passes of a request leave no room for answers `serverUnavailable`
with nothing stored. On a Gmail account the folder is the store the
row was fetched from. A flag the PERMANENTFLAGS of the folder do not
name is never sent: a system flag answers `forbidden`, a keyword
without `\*` there `invalidProperties` with the path as the patch
wrote it. A folder the server opens `[READ-ONLY]` answers `forbidden`
and is sent nothing past its SELECT. A SELECT that names more than 1024
permanent flags is read as a protocol failure; an EXAMINE passes the
list over. What the server took is written as one state: the keywords
and memberships of each row that differs, the rows that moved under
their new folder and UID and the rows that left. Each such email reads
as updated or destroyed, a thread a destroy changed as updated or
destroyed and every mailbox whose counts moved as updated; `oldState`
and `newState` are the states around that write. An update whose email
the server left otherwise than its patch asked answers the properties
that differ (RFC 8620 section 5.3). A command the server refuses
answers `serverFail` for its updates and keeps the session; an answer
the bridge cannot read answers `serverFail` too and costs the session;
a server out of reach, a lost connection, the deadline, a folder
renumbered since the sync and a row no UID names answer
`serverUnavailable`. None of them writes a row, while a set of messages
the server took before the failure is kept. An update that both sets
and clears keywords is two commands; a failure between them leaves the
first on the server until the next refresh reads it. An update that
names more than 4 KiB of keywords answers `tooManyKeywords`. The
refresh after a write finds every row where the write left it and logs
no email; the mailbox pass logs the folders, since their counts moved.

The session layer also carries what moving and deleting mail takes,
each answer read under the guard and the line bounds like any other.
`uid_move` sends `UID MOVE` (RFC 6851) and `uid_copy` sends `UID COPY`
to a mailbox by its wire name; both answer the pairs of old and new
UIDs the `COPYUID` code names (RFC 4315 section 3). `uid_expunge`
stores `\Deleted` on the messages it names, then sends `UID EXPUNGE` of
the same set, so a message another client marked stays; when the STORE
fails the expunge is never sent. `\Deleted` is stored there alone and no
plain EXPUNGE is ever built. `quota_root` sends `GETQUOTAROOT INBOX`
(RFC 9208) and answers each quota root with its storage in octets and
its message count. A `COPYUID` whose two sets do not pair is a protocol
failure, as is one that names a UID the ask did not or a zero; a UID
that names no message of the folder is passed over and has no pair. A
QUOTA value past 63 bits, a storage past 64 bits in octets and a root
name that cannot be sent back are protocol failures too. Every write
needs a mailbox a SELECT opened for writing: after an EXAMINE, or a
SELECT the server answered with `[READ-ONLY]`, the session refuses it
before anything is sent. `Email/set` sends the moves and the expunge;
the mailbox pass sends the quota root where QUOTA is advertised, last
in a refresh, so a quota the server cannot describe costs no folder.

`uid_search` asks `UID SEARCH` with criteria over the same windows of
sequence numbers as the UID list, newest first (RFC 3501 section
6.4.4): a TEXT key per term of the text and a FROM key per term of the
sender, a run in double quotes one term, then UNSEEN, FLAGGED, SINCE
and BEFORE. Every string of the client goes on the line as a literal
with its byte count, never bare or quoted. It goes without waiting where
the server advertises LITERAL+ or, up to 4096 bytes, LITERAL- (RFC
7888); otherwise it waits for the continuation request. A term past 256
bytes, more than eight terms or a NUL is refused before anything is
sent (`SearchAsk::check`). On Gmail the text and the sender go as one
X-GM-RAW string and a label as X-GM-LABELS. `CHARSET UTF-8` goes out
once per command, right after `UID SEARCH`, when the command carries a
literal. A NO with the `BADCHARSET` code reads as
`SessionError::BadCharset`. `Capabilities::searching` says which form
the literals take and whether the Gmail keys apply. With `until` set,
each read of the answer waits until then in place of the session's
step.

An `Email/query` with a text or a sender has the server searched
between two passes over its request, on a connection of the account's
own, one search at a time. A folder account searches the folders of
the scope in the tree's order with Junk and Trash last. A Gmail account
searches All Mail, under the label of a label mailbox; Spam and Trash
run where the scope keeps them. The searches of one round of a request
share one deadline (`runtime::SEARCH_DEADLINE`, forty-five seconds),
counted from the first of them, their wait behind another search
included. Past it a call answers `serverUnavailable` without
connecting; a search it cuts short drops its connection, as one given
up on does. A folder the server refuses or shows under another
UIDVALIDITY than its rows is passed over. The hits stay in memory per
account under the search they answer, four at most for five minutes
(`runtime::SEARCH_CACHE_ENTRIES`, `runtime::SEARCH_CACHE_TTL`), so the
pages after the first cost no SEARCH. SINCE and BEFORE go a day wider
each side, since a server compares the day of an internal date as
written; the dates, the keywords, the attachment and the scope are exact
in SQL over the hits. No query text and no sender is logged.

The rows live in `bridge_` tables inside the host's database.
`store::MIGRATIONS` carries their schema for the host's migration list
as two migrations, the tables and then the index on the message ids by
thread with the progress columns of the refresh; every row carries the
host's opaque account key. Ids are a type letter in front of UUID text.
The state string is a per-account counter; every change writes a row
to a log that the three `/changes` methods read, kept for the newest
ten thousand rows. `jmap::handle` runs a Request object (RFC 8620
section 3.3) against those rows and answers `Mailbox/get`,
`Mailbox/changes`, `Email/get`, `Email/set`, `Email/query`,
`Email/changes`, `Thread/get`, `Thread/changes`, `Quota/get`,
`Quota/changes` and `Core/echo`.
`Email/get` serves the
metadata, the header properties and the preview from the rows, the
body properties and the `header:` forms from the server. `Email/query`
serves one filter condition: `inMailbox`, `inMailboxOtherThan` or
neither, which is the whole account, with `hasKeyword`, `notKeyword`,
`after`, `before`, `hasAttachment`, `text` and `from`. The sort is
`receivedAt` either way, the window by position or by anchor, with the
total and one email per thread on request, two hundred ids at most
(`jmap::QUERY_LIMIT`). A filter without a text and a sender answers from
SQL alone. A term past the caps or holding a NUL answers
`invalidArguments` before anything connects, a server that cannot search
in UTF-8 `unsupportedFilter`; any other filter or sort answers
`unsupportedFilter` or `unsupportedSort`. Result references
resolve under a budget of one MiB per request, measured as the memory
a copy of the resolved values takes; a reference past it answers
`invalidResultReference`. `jmap::session_object` renders the session
object with the core, mail and quota capabilities and the vendor
capability `https://huliho.com/jmap`, which carries the Mailbox
property `syncedEmails` and is followed only when a request names it.
The account lists the quota capability where the server advertises
QUOTA. The host supplies the URLs. `Sealer` is the trait behind which
the host keeps its cryptography for the personal fields of an email
row; the sync and `jmap::handle` reach it through `sync::Cache`.

The quota the last pass read stays in memory per account: no table
holds it. Its state is a counter behind a mark drawn when the store
opens, so a state from before a restart is unknown. `Quota/get`
(RFC 9425) answers one quota per resource of a root, STORAGE in
`octets` and MESSAGE as a `count`; each counts `Email` in the scope
`account`. A request without the mail capability sees none.
`Quota/changes` knows the state the account stands at and answers
`cannotCalculateChanges` for any other; `Quota/query` and
`Quota/queryChanges` are not served. An account whose server has no
QUOTA answers `accountNotSupportedByMethod`.

Other software reuses the bridge by running it as its own process and
speaking JMAP to it. Linking the crate into a program instead creates a
combined work, which must ship under the AGPL (see LICENSE and NOTICE).

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`. `cargo test -p huliho-imap-bridge --features
live-targets` adds the tests that need the compose Dovecot.

The `test-support` feature exposes `testing`, the scripted IMAP and
SMTP servers the tests run against: a fresh certificate per server, one
answer per command and a record of every line received. Its IMAP script
carries a mailbox model with LIST-EXTENDED, LIST-STATUS, SPECIAL-USE,
CONDSTORE, MOVE, UIDPLUS and QUOTA as switches and a message model
behind EXAMINE, SELECT
with the PERMANENTFLAGS of the folder, NOOP, UID SEARCH, UID STORE and
UID FETCH, the flags alone with CHANGEDSINCE, the two
sections of a preview cut as the partial fetch asks, the structure with
a set of header fields, the fields alone and one window of one section
or of the whole message
included. UID MOVE and UID COPY give the messages new UIDs in the
target and name them in `COPYUID`, UID EXPUNGE removes what its set
marked `\Deleted`, GETQUOTAROOT answers the lines a test writes (an
inbox under no root by default; a refusal on request) and a folder can
open `[READ-ONLY]` or show another
UIDVALIDITY on one SELECT. UID SEARCH reads its window of sequence
numbers with the keys CHARSET, TEXT, FROM, UNSEEN, FLAGGED, SINCE and
BEFORE; a string comes as an atom, quoted or as a literal. The Gmail
mode adds X-GM-RAW and X-GM-LABELS. LITERAL+ and LITERAL- are switches
too: the Dovecot shape carries LITERAL+, the Gmail mode LITERAL-. A
literal the script does not read without its continuation request is
read whole and answered BAD. Misbehavior is a switch as well:
volunteered lines, a connection that drops, a MODSEQ item, a NO in raw
UTF-8, messages that leave between two searches, the Gmail items
volunteered unasked, a STORE or a move that is refused, answered BAD,
dropped or never answered, a `COPYUID` of the test's own words, a
continuation request withheld or refused, a search in one charset
alone, a SEARCH line padded to a size, a SEARCH answered late plus an
EXAMINE answer without EXISTS. `Mailboxes::searches` counts the UID
SEARCH commands every connection received. Its `Mailboxes`
stand in for a second client between two passes: a message appended,
one expunged by UID, the flags of one replaced, the counts and the
mod-sequences following. Its Gmail mode
(`Mailboxes::gmail`) lists the folders as Gmail does, answers the three
Gmail items where asked and BAD where the extension is off, takes a
STORE of `X-GM-LABELS` with the Starred label and `\Flagged` kept
together, counts a label folder from the labels of All Mail's messages
and lets a test relabel a message or move it between stores as a
second client would.
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
a mismatch the test reads. Both read a command's literals by their size,
so one holding a line break stays one command and its markers survive
the redaction. Gmail's transcript is
`tests/transcripts/gmail.json`, replayed by the scenario suite in
`tests/gmail_transcript.rs` in every test run; the same suite runs
against the scripted server's Gmail mode through the recorder and back
through the replayer, and against a live test account when
`HULIHO_LIVE_GMAIL_ADDRESS` and `HULIHO_LIVE_GMAIL_APP_PASSWORD` are
set, writing the transcript to the path in `HULIHO_RECORD_TRANSCRIPT`
when that is set as well. The checked-in transcript and the one
recorded off the scripted server both fail on a `\Deleted` or an
expunge sent outside Spam and Trash. `tests/transcripts.rs` scans
every checked-in transcript.
