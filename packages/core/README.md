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
the state of every member, the pages of each list, the searches it holds
in memory alone, message bodies, the changes the server has not
acknowledged, the drafts, the account's quotas and one state per object
type, plus the senders its headers name), written in batches that land
whole;
`MemoryMailStore` is the one that keeps everything in memory. On top of
both, `syncMailboxes`
brings the mailbox tree up to date, `queryWindow` serves a page of a
list and fetches it when the store lacks it, `applyChanges` follows the
change logs of every type, `revealNewMail` lands the new mail a poll
held back for the user and `readThread` answers a thread with the
headers the store holds. `searchWindow` serves a page of a search from
the one condition `searchFilterOf` builds from its params and the
account's mailboxes, every mailbox but Junk and Trash unless the params
name one or all of them. The held pages read end to end over the
store's rows, one row per thread: no poll adds a row, a thread leaves
once none of its members stands in the scope and the rows after it
close up. A page they leave short, or a later page asked first, takes
the server's next pages by anchor on the last row shown; the total
drops by the threads that show no row and is exact once the server has
no more. Each row acts from one mailbox, the scope's own or else the
inbox or the first in the tree's order. Four searches stay per
account. `parseQuery` reads the search
field's text into those params with its operators such as `from:` and
`in:`; `queryOf` writes them back; `readSenders` offers the senders of
the held headers that match what the user typed. `readBody` answers the body of one email: the
stored row, else one request whose answer is stored; the bodies fetched
longest ago leave past 500 rows or 64 MiB. `mutateMany` takes changes
to the keywords and mailboxes of emails into the rows at once and logs
them, answering the change that takes each one back. An email the
store holds no header for is read from its thread's member state, so a
thread acts whole from its row in a list. A thread that
leaves a mailbox leaves its list at once and an undo puts it back where
it stood; a thread that enters a mailbox waits for the next fetch of
that list. A delete forever hides the email at once and waits in the
log until `releaseHeld` lets it go or `dropHeld` takes it back;
`releaseLapsed` lets go of the ones whose tab holds no lease.
`flushPending` sends one round of the log as one `Email/set` with its
updates and destroys, drops what the server acknowledged and takes back
what it refused; rows that add up to nothing leave without a request.
Rows that land from the server while a change waits take that change
again, so a poll leaves a message the user read as read and an archived
thread out of its list. `mayPatch` says whether the account and the
mailboxes involved take a change at all, so one the server said it
refuses is never logged. `runJob` marks a whole mailbox read or empties
it, one `jobRound` at a time. `readQuota` answers the quota a bar shows,
which the mailbox pass keeps current where the account has quota.
`setMailbox` writes the account's folders: one `Mailbox/set` with its
creates, updates and destroys goes out in front of a poll's request,
so what the server wrote lands in the store before the call resolves,
the emails and lists a destroy took included; each object the server
refused comes back with its SetError. `visibleMailboxes` names the
mailboxes the tree draws: the subscribed ones and the roles plus the
folders above a subscribed folder, every mailbox where the account
subscribes none; `branchOf` names a folder with the folders without a
role under it and `hideWrite` the write that takes that branch out of
the tree. The session keeps what the
account allows a folder, a create at the top and the longest name.
`offeredActions` reads what a mailbox offers from its role and the
rights of the mailboxes involved, never from the kind of account, the
mailboxes a move may enter among it; `offeredJobs` reads which folder
jobs a mailbox offers. `actionMutations` turns an action on a thread
into the changes of its emails in that mailbox, `moveMutations` does
so for a move to the mailbox picked and `verbOf` names what a refused
change did.
`MailCache` is what an adapter offers the query hooks: the tree, what
the account allows a folder, one write of its folders, a page, a
thread, the reveal, the
body of one email with where its parts download from, one change to
an email or the changes to several with the release or the drop of a
held destroy, the folder jobs and the quota, every read answered from
the cache. A store that keeps rows on
disk checks them against the row schemas when they come back.

It keeps the drafts too. A draft's row is the working copy of the
account's draft on the server. `saveDraft` writes the composer's change
into it; `writeDraft` sends it as one `Email/set` that creates the new
copy in Drafts and destroys the old one, after which the row names its
files by the parts of the new copy. `queueDraft` holds a send for
`SEND_UNDO_MS` and `undoSend` takes it back inside that window.
`drainDrafts` sends what waited past it as an `EmailSubmission/set`
that files the draft in Sent: one draft at a time, a send whose answer
was lost read from the server first, the parent of a reply or a forward
marked as answered or forwarded. A refusal turns the row back into a
draft with its cause. `discardDraft` destroys a draft, `readIdentity`
reads the identity a draft goes out as and `uploadFile` sends a file to
the account's upload route with its progress.

The compose helpers build what a draft carries. `createObject` writes
the create with its `bodyStructure`, the text part in format=flowed
where the server keeps the parameters, else one line per paragraph.
`writeFlowed` writes that text (RFC 3676) from the lines
`documentLines` reads out of the composer's document. The quote
builders turn the parent's body into a reply's or a forward's quote.
`replyHeaders`, `answerSubject` and `replyRecipients` give what a reply
answers. `tokenize` reads a pasted list of addresses and `readMailto` a
`mailto:` link; `guardrailOf` and `refusalOf` say what holds a send
back.

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
