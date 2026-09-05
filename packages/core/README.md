# @huliho/core

Domain types, the JMAP client and sync logic, free of React and DOM
APIs so every client can share it. Today it holds the Huliho session
boundary: sign-in, sign-out and the current session, plus the session
list with its revokes, the password change and the admin's users with
create and reset. Every answer from the server passes a zod schema
before it reaches a caller.

`pnpm build` at the repo root compiles it; unit tests run with
`pnpm test`.
