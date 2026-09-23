# @huliho/core

Domain types, the JMAP client and sync logic, free of React and DOM
APIs so every client can share it. Today it holds the Huliho session
boundary: sign-in, sign-out and the current session, plus the session
list with its revokes, the password change, the admin's users with
create and reset and the connected accounts with their discovery,
connect, reconnect, retry and removal plus the consent a Google or
Microsoft account signs in through, started once and polled until it
settles. Every
answer from the server passes a zod
schema before it reaches a caller; an address is checked against the
server's shape rule before it goes out.

It also holds the read side of the mail cache. `JmapClient` speaks to
one account's endpoint on the proxy: the session object, then Request
objects whose method calls it sends in one round trip. `MailStore` is
the contract a store implements (mailboxes, email headers, threads with
the state of every member, the pages of each list and one state per
object type), written in batches that land whole; `MemoryMailStore` is
the one that keeps everything in memory. On top of both, `syncMailboxes`
brings the mailbox tree up to date, `queryWindow` serves a page of a
list and fetches it when the store lacks it, `applyChanges` follows the
change logs of every type and `revealNewMail` lands the new mail a poll
held back for the user.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
