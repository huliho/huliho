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

Configuration is one TOML file; unknown keys are rejected. Top-level
keys are the `listen` address and the `assets` directory; `[storage]`
holds the data directory `path` (default `data`) and `[events]` holds
the event log `retention_days` (default 365). The file path comes from
`HULIHO_CONFIG` and that
file must exist. Without the variable the server reads `huliho.toml`
from the working directory and falls back to the defaults when it is
absent.

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`.
