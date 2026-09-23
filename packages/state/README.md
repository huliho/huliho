# @huliho/state

TanStack Query hooks on top of @huliho/core, including the query-key
registry. Today it holds the key registry and the query options for
the current session, the session list, the users, the connected
accounts and the poll of a pending consent.

The mail queries read the cache rather than the network: each one
takes the `MailCache` an adapter supplies (the web app's worker, a
mobile app's own store) as its first argument and stays fresh until a
cache message invalidates it. Their keys start with the account id, so
`[accountId, "mailboxes"]`, `[accountId, "window", mailboxId, page]`
and `[accountId, "thread", threadId]` share a prefix per account and
the words in second place mark a key as a mail key.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
