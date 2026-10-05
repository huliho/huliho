# @huliho/core

Domain types, the JMAP client and sync logic, free of React and DOM
APIs so every client can share it. Today it holds the Huliho session
boundary: sign-in, sign-out and the current session, plus the session
list with its revokes, the password change, the admin's users with
create and reset and the connected accounts with their discovery,
connect, reconnect, retry and removal plus the consent a Google or
Microsoft account signs in through, started once and polled until it
settles, plus the user's preferences: the words each key takes (the
locale, the theme, the density, the reading pane, the font size, the
line height and how a light-only message shows in the dark theme) and
one write per key. The per-sender policies sit beside them: the list,
the grant that lets a sender's remote content load and its removal.
Every answer from the server passes a zod
schema before it reaches a caller; an address is checked against the
server's shape rule before it goes out.

It also holds the mail cache. `JmapClient` speaks to
one account's endpoint on the proxy: the session object, then Request
objects whose method calls it sends in one round trip. `MailStore` is
the contract a store implements (mailboxes, email headers, threads with
the state of every member, the pages of each list, message bodies, the
changes the server has not acknowledged and one state per object
type), written in batches that land whole; `MemoryMailStore` is
the one that keeps everything in memory. On top of both, `syncMailboxes`
brings the mailbox tree up to date, `queryWindow` serves a page of a
list and fetches it when the store lacks it, `applyChanges` follows the
change logs of every type, `revealNewMail` lands the new mail a poll
held back for the user and `readThread` answers a thread with the
headers the store holds. `readBody` answers the body of one email: the
stored row, else one request whose answer is stored; the bodies fetched
longest ago leave past 500 rows or 64 MiB. `applyPatch` takes a change
to an email's keywords into the rows at once and logs it;
`flushPending` sends one round of the log as one `Email/set`, drops
what the server acknowledged and takes back what it refused. Rows that land
from the server while a change waits take that change again, so a poll
leaves a message the user read as read. `mayPatch` says whether the
account and the mailboxes of an email take a change at all, so one the
server said it refuses is never logged. `MailCache` is what an adapter
offers the query hooks: the tree, a page, a thread, the reveal, the
body of one email with where its parts download from and one change to
an email, each answered from the cache. A store that keeps rows on
disk checks them against the row schemas when they come back.

It reads what a message says about itself as well. `unflow` joins the
soft line breaks of a flowed text part (RFC 3676) and `quotedLines`
reads the quote depth of a plain one line by line. The topmost
Authentication-Results header becomes one verdict per method (RFC
8601), from a parser with a bound on bytes and clauses; `grantLoads`
decides from it whether a sender's grant lets a message load remote
content. `classifyImageUrl` and `classifyLinkUrl` name what an image
URL and a link's target in a mail are, by the policy the server's
sanitizer holds as well; `linkText` is a link's text as a reader sees
it and `linkRisk` names why a link asks the reader before it opens.
`downloadUrl` fills in the session's download template for one blob
(RFC 8620 section 6.2).

The manifest names the one module with a side effect on import, the
one that configures zod, so a bundle carries only what it uses.

`@huliho/core/testing` exports the JMAP server the cache tests run
against, so an adapter's tests can drive the same reconciliation.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
