## What

Describe the change in one or two sentences.

## Dependencies

For every new dependency: why it is needed, that it is actively
maintained and which license it ships under. For new utility code over
50 lines, name the libraries considered and why they were not used.
Write "no new dependencies" otherwise.

## Security callout

Fill this in when the change touches auth, sessions, mail rendering,
sanitizing, headers, the proxy or protocol parsing: what changed and
which abuse cases the tests cover. Write "not security-relevant"
otherwise.

## Simplicity

Confirm the change carries no abstraction that pays for itself only once,
no generic built for a second caller that does not exist and no option for
something that never varies. Say which parts were extracted or dropped to
keep it that way.

## UI states

For every new or changed view: screenshots of all applicable states
(default, loading, first sync, empty, error, offline, RTL) at phone
width and at desktop width. Write "no UI change" otherwise.
