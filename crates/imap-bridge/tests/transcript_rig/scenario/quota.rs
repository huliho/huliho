// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The quota step: the root Gmail names for the inbox, one storage root
//! without a name, read through `Quota/get` as the account's one quota
//! over its email.

use serde_json::json;

use super::super::ACCOUNT;
use super::Scenario;

impl Scenario {
    /// The mailbox pass read the quota root; `Quota/get` answers it.
    pub(super) async fn quota(&mut self) {
        let arguments = json!({ "accountId": ACCOUNT, "ids": null });
        let found = self.call("Quota/get", arguments).await;
        let list = found["list"].as_array().unwrap();
        assert_eq!(list.len(), 1, "{found}");
        let storage = &list[0];
        assert_eq!(storage["resourceType"], "octets", "{found}");
        assert_eq!(storage["name"], "");
        assert_eq!(storage["scope"], "account");
        assert_eq!(storage["types"], json!(["Email"]));
        let used = storage["used"].as_u64().unwrap();
        let limit = storage["hardLimit"].as_u64().unwrap();
        assert!(used <= limit && limit > 0, "{found}");
    }
}
