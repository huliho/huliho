# huliho-server

The Huliho server binary. One process serves the built web app and the
API: static assets with an SPA fallback, `/healthz` for liveness, the
security headers on every response, request-scoped tracing and a
graceful shutdown on SIGTERM.

All persistent state lives in an embedded database inside one data
directory, created on first start and migrated on every start. The
schema covers organizations, users with fixed roles and a display
name, connected accounts, server-side sessions with a device record
and the address of their last use, an append-only domain event log
with configurable retention, per-user preferences and per-sender
policies. Every read and mutation requires a typed scope from the
single resolver, so nothing reaches storage across an organization or
user boundary. A signed-in user lists their own sessions and ends any
of them but the current one; every mutation stamps its session at most
once per five minutes and records the user as active for the month.
A user changes their own password against the current one; every
other session ends and the current one continues on a fresh token.
An admin creates users and resets passwords through a one-time
password that is shown once, works for one sign-in within a day and
opens a session that reaches only the password change. A user's
connected accounts are rows carrying the address, a display name, the
provider preset and the connection settings; the credential sits beside
them sealed under a key of its own and bound to the row, so it never
reaches the browser. A signed-in user lists their own accounts and
removes any of them; the credential leaves with the row.
Adding an account starts with discovery: the server takes a mail
address and answers the server behind it. The well-known mail domains
name their preset without a lookup; every other domain runs a chain of
the JMAP well-known resource and its SRV record, the RFC 6186 SRV
records, the Thunderbird autoconfig documents and the MX names of the
known providers, in that order, each step within five seconds and all
of them within fifteen. Every outbound connection resolves through one
resolver that refuses private networks unless the config lists them,
follows at most three redirects over HTTPS to named hosts only and
validates certificates against the built-in roots plus the configured
CA file. Discovery counts against the sign-in rate limiter.

Configuration is one TOML file; unknown keys are rejected. Top-level
keys are the `listen` address, the `assets` directory and the optional
`public_url`, the base URL the instance is reached on. `[storage]`
holds the data directory `path` (default `data`) and `[events]` holds
the event log `retention_days` (default 365). `[auth]` holds the
`secret_file` path plus the session `idle_timeout_minutes` and
`absolute_timeout_minutes`. `[upstream]` holds the rules for reaching
mail servers. `allow_private_networks` lists the private networks an
upstream may resolve to, written as CIDRs. It is empty by default.
`additional_ca_file` names one PEM bundle trusted next to the built-in
roots. `probe_interval_minutes` says how often a stopped account is
checked for recovery, fifteen by default. Discovery reads the network
rules and the CA file; the account list reports the probe interval,
which nothing acts on yet. The file path comes from `HULIHO_CONFIG` and
that file must exist. Without the variable the server reads
`huliho.toml` from the working directory and falls back to the defaults
when it is absent.

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`. `cargo test -p huliho-server --features
live-targets` adds the tests that need the compose targets and the
public internet.
