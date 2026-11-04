// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The draft steps of the first client: a draft saved to the Drafts
//! label, replaced with the old copy moved to Trash, then destroyed the
//! same way, with no `\Deleted` and no expunge anywhere near All Mail.

use huliho_imap_bridge::testing::record::fixed::FIXED_DOMAIN;
use serde_json::{Value, json};

use super::super::ACCOUNT;
use super::super::corpus::DRAFT_SUBJECT;
use super::Scenario;

impl Scenario {
    /// Every draft step in order.
    pub(super) async fn drafts(&mut self) {
        let mailbox = self.suite.by_role("drafts").id.to_string();
        let first = self.saved(&mailbox, None).await;
        let shown = self
            .emails(
                std::slice::from_ref(&first),
                &["subject", "keywords", "mailboxIds"],
            )
            .await
            .remove(0);
        assert_eq!(shown["subject"], DRAFT_SUBJECT, "{shown}");
        assert_eq!(shown["keywords"]["$draft"], true, "{shown}");
        let members = Self::mailbox_ids(&shown);
        assert!(members.contains(&mailbox), "{shown}");
        let second = self.saved(&mailbox, Some(&first)).await;
        assert_ne!(first, second);
        let gone = json!({ "accountId": ACCOUNT, "ids": [&first], "properties": ["id"] });
        let gone = self.call("Email/get", gone).await;
        assert_eq!(gone["notFound"], json!([first]), "{gone}");
        let destroy = json!({ "accountId": ACCOUNT, "destroy": [&second] });
        let destroyed = self.call("Email/set", destroy).await;
        assert_eq!(destroyed["destroyed"], json!([second]), "{destroyed}");
    }

    /// One save through `Email/set`, replacing an earlier one where named.
    /// The id it answers.
    async fn saved(&self, mailbox: &str, old: Option<&str>) -> String {
        let address = format!("sanne@{FIXED_DOMAIN}");
        let draft = json!({
            "mailboxIds": { (mailbox): true },
            "keywords": { "$draft": true, "$seen": true },
            "from": [{ "email": address }],
            "to": [{ "email": format!("mo@{FIXED_DOMAIN}") }],
            "subject": DRAFT_SUBJECT,
            "bodyStructure": { "partId": "t", "type": "text/plain" },
            "bodyValues": { "t": { "value": "Body of the draft.\n" } },
        });
        let mut arguments = json!({ "accountId": ACCOUNT, "create": { "d": draft } });
        if let Some(old) = old {
            arguments["destroy"] = json!([old]);
        }
        let answer = self.call("Email/set", arguments).await;
        if let Some(old) = old {
            assert_eq!(answer["destroyed"], json!([old]), "{answer}");
        }
        assert_eq!(answer["notCreated"], Value::Null, "{answer}");
        let made = &answer["created"]["d"];
        assert_eq!(made["blobId"], made["id"], "{answer}");
        assert!(made["size"].as_u64().unwrap() > 0, "{answer}");
        made["id"].as_str().unwrap().to_owned()
    }
}
