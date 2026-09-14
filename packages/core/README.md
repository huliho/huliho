# @huliho/core

Domain types, the JMAP client and sync logic, free of React and DOM
APIs so every client can share it. Today it holds the Huliho session
boundary: sign-in, sign-out and the current session, plus the session
list with its revokes, the password change, the admin's users with
create and reset and the connected accounts with their discovery,
connect and reconnect plus the consent a Google or Microsoft account
signs in through, started once and polled until it settles. Every
answer from the server passes a zod
schema before it reaches a caller; an address is checked against the
server's shape rule before it goes out.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
