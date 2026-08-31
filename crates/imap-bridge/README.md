# huliho-imap-bridge

Translates JMAP Mail semantics (RFC 8620, RFC 8621) to IMAP4rev2
(RFC 9051) and SMTP submission (RFC 6409), so the rest of Huliho only
speaks JMAP. The crate holds no code yet.

Other software reuses the bridge by running it as its own process and
speaking JMAP to it. Linking the crate into a program instead creates a
combined work, which must ship under the AGPL (see LICENSE and NOTICE).

Build and test from the workspace root: `cargo build` and
`cargo test --workspace`.
