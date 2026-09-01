# huliho-server

The Huliho server binary. One process serves the built web app and the
API: static assets with an SPA fallback, `/healthz` for liveness, the
security headers on every response, request-scoped tracing and a
graceful shutdown on SIGTERM.

Configuration is one TOML file holding the `listen` address and the
`assets` directory; unknown keys are rejected. The path comes from
`HULIHO_CONFIG` and that file must exist. Without the variable the
server reads `huliho.toml` from the working directory and falls back
to the defaults when it is absent.

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`.
