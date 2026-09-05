// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A resolver that answers what a test puts in it and records every
//! lookup.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

use huliho_server::upstream::{Dns, Lookup, SrvTarget};

/// Records by name; a name that is absent answers nothing, like a name
/// without records.
#[derive(Default)]
pub struct FakeDns {
    pub addresses: HashMap<String, Vec<SocketAddr>>,
    pub srv: HashMap<String, Vec<SrvTarget>>,
    pub mx: HashMap<String, Vec<String>>,
    /// Every lookup made, as `a name`, `srv name` or `mx name`.
    pub queries: Mutex<Vec<String>>,
}

impl FakeDns {
    fn answer<T: Clone + Send + 'static>(
        &self,
        kind: &str,
        name: &str,
        table: &HashMap<String, Vec<T>>,
    ) -> Lookup<'_, Vec<T>> {
        self.queries.lock().unwrap().push(format!("{kind} {name}"));
        let records = table.get(name).cloned().unwrap_or_default();
        Box::pin(std::future::ready(Ok(records)))
    }
}

impl Dns for FakeDns {
    fn addresses<'a>(&'a self, host: &'a str) -> Lookup<'a, Vec<SocketAddr>> {
        self.answer("a", host, &self.addresses)
    }

    fn srv<'a>(&'a self, service: &'a str) -> Lookup<'a, Vec<SrvTarget>> {
        self.answer("srv", service, &self.srv)
    }

    fn mx<'a>(&'a self, domain: &'a str) -> Lookup<'a, Vec<String>> {
        self.answer("mx", domain, &self.mx)
    }
}
