// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where a session connects and how long one step may take.

use std::net::SocketAddr;
use std::time::Duration;

/// One connect attempt or one step of a command gets this long: the
/// whole command where the client library runs it, each response where
/// the bridge reads the answer itself. A slower server counts as
/// unreachable.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(20);

/// How a connection is encrypted; plaintext is not an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// TLS from the first byte.
    Implicit,
    /// A plaintext greeting, then STARTTLS before any credential.
    Starttls,
}

/// A server to reach. The caller resolves the host and checks the
/// addresses; the bridge connects to them in order and resolves nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The name the certificate must carry, sent as SNI.
    pub host: String,
    /// Where to connect, port included, tried in order.
    pub addresses: Vec<SocketAddr>,
    /// Whether TLS starts at the first byte or after STARTTLS.
    pub tls: TlsMode,
}
