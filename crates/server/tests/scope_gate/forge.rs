// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use huliho_server::ids::Role;
use huliho_server::scope::Scope;

fn main() {
    let _scope = Scope {
        organization_id: "org".to_owned().into(),
        user_id: "user".to_owned().into(),
        role: Role::Member,
        account_id: None,
    };
}
