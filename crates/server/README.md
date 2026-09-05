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
opens a session that reaches only the password change.

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
checked for recovery, fifteen by default. Nothing reads the upstream
rules yet. The file path comes from `HULIHO_CONFIG` and that file must
exist. Without the variable the server reads `huliho.toml` from the
working directory and falls back to the defaults when it is absent.

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`.
