// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The store survives a reopen with its data intact.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use huliho_server::identity;
use huliho_server::scope;
use huliho_server::store::Store;

fn scratch_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is past the epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("huliho-store-test-{}-{nanos}", std::process::id()))
}

#[test]
fn a_reopened_store_keeps_its_data() {
    let dir = scratch_dir();
    let user_id = {
        let store = Store::open(&dir).expect("store opens");
        let (_, user) =
            identity::create_personal_user(&store, "mira@example.com").expect("user creates");
        user.id
    };
    let store = Store::open(&dir).expect("store reopens");
    let resolved = scope::resolve(&store, &user_id, None).expect("scope resolves after reopen");
    assert_eq!(resolved.user_id(), &user_id);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
