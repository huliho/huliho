# @huliho/state

TanStack Query hooks on top of @huliho/core, including the query-key
registry. Today it holds the key registry and the query options for
the current session, the session list, the users, the connected
accounts, the poll of a pending consent, the user's preferences and
the per-sender policies.

The mail queries read the cache rather than the network: each one
takes the `MailCache` an adapter supplies (the web app's worker, a
mobile app's own store) as its first argument and stays fresh until a
cache message invalidates it. A mail query runs whatever the network
does, since the cache answers from what it holds; one that failed asks
again when the network comes back. Their keys start with the account id, so
`[accountId, "mailboxes"]`, `[accountId, "window", mailboxId, page]`,
`[accountId, "thread", threadId]`, `[accountId, "body", emailId]`,
`[accountId, "quota"]`, `[accountId, "search", key, locale, page]` and
`[accountId, "senders", prefix]` share a prefix per account and the
words in second place mark a key as a mail key. A search stays as the
server answered it until the user runs it again.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
