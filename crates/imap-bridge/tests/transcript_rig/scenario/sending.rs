// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The send of the first client: the one identity read, then a draft
//! saved from its address to the account's own address goes out through
//! `EmailSubmission/set` with the patch that files it. On Gmail the
//! draft goes to Trash with UID MOVE and nothing is expunged, since
//! Gmail files its own copy under the Sent label; once a refresh shows
//! that copy, the label holds it alone.

use huliho_imap_bridge::testing::Message;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use serde_json::{Value, json};

use super::super::ACCOUNT;
use super::super::corpus::DRAFT_SUBJECT;
use super::super::gmail::SETTLE;
use super::drafts::draft;
use super::moving::Store;
use super::{LISTING, LOOKS, Scenario, Second};

/// The label Gmail puts on the copy it files of what it sent.
const SENT_LABEL: &str = "\\Sent";

impl Scenario {
    /// The send and what it leaves, in order.
    pub(super) async fn sends(&mut self) {
        let drafts = self.suite.by_role("drafts").id.to_string();
        let sent = self.suite.by_role("sent").id.to_string();
        let address = self.suite.address().to_owned();
        self.one_identity(&address).await;
        let draft = self.saved(draft(&drafts, &address, &address), None).await;
        let message_id = self.message_id_of(&draft).await;
        let before = self.suite.state();
        let filing = json!({
            (format!("mailboxIds/{drafts}")): null,
            (format!("mailboxIds/{sent}")): true,
            "keywords/$draft": null,
        });
        let response = self.submitted(&draft, filing).await;
        let responses = response["methodResponses"].as_array().unwrap();
        assert_eq!(responses.len(), 2, "{response}");
        let made = &responses[0][1]["created"]["s"];
        assert_eq!(made["undoStatus"], "final", "{response}");
        assert_eq!(responses[1][0], "Email/set", "{response}");
        assert_eq!(responses[1][2], "c1", "{response}");
        let implicit = &responses[1][1];
        assert_eq!(
            implicit["updated"],
            json!({ (draft.as_str()): null }),
            "{response}"
        );
        assert_eq!(implicit["notUpdated"], Value::Null, "{response}");
        let gone = json!({ "accountId": ACCOUNT, "ids": [&draft], "properties": ["id"] });
        let gone = self.call("Email/get", gone).await;
        assert_eq!(gone["notFound"], json!([&draft]), "{gone}");
        assert!(
            self.second_finds_id(&message_id, Store::Trash.alone(), true)
                .await
        );
        self.sent_copy_filed(&message_id, &address);
        assert_eq!(self.sent_copies(before).await, 1);
        let drafts_left = self.subjects_in("drafts").await;
        assert!(
            !drafts_left.iter().any(|subject| subject == DRAFT_SUBJECT),
            "{drafts_left:?}"
        );
    }

    /// The one identity the send goes out under: the account's address
    /// with an empty name, which no call deletes (RFC 8621 section 6).
    async fn one_identity(&self, address: &str) {
        let arguments = json!({ "accountId": ACCOUNT, "ids": null });
        let answer = self.call("Identity/get", arguments).await;
        let list = answer["list"]
            .as_array()
            .unwrap_or_else(|| panic!("{answer}"));
        assert_eq!(list.len(), 1, "{answer}");
        assert_eq!(list[0]["email"], address, "{answer}");
        assert_eq!(list[0]["name"], "", "{answer}");
        assert_eq!(list[0]["mayDelete"], false, "{answer}");
    }

    /// The Message-ID of the email in its header form, angle brackets
    /// around it, as the second client searches for it.
    async fn message_id_of(&self, id: &str) -> String {
        let shown = self
            .emails(&[id.to_owned()], &["messageId"])
            .await
            .remove(0);
        format!("<{}>", shown["messageId"][0].as_str().unwrap())
    }

    /// `EmailSubmission/set` of the draft with the patch given for it on
    /// success; the whole Response object.
    async fn submitted(&self, draft: &str, filing: Value) -> Value {
        let arguments = json!({
            "accountId": ACCOUNT,
            "create": { "s": { "identityId": ACCOUNT, "emailId": draft } },
            "onSuccessUpdateEmail": { "#s": filing },
        });
        self.suite
            .response(&json!(["EmailSubmission/set", arguments, "c1"]))
            .await
    }

    /// Gmail files a copy of what it sent under its Sent label: the
    /// model gets one with the draft's subject and Message-ID, the live
    /// account files its own and nobody plays it behind a transcript.
    fn sent_copy_filed(&mut self, message_id: &str, address: &str) {
        let Second::Model(mailboxes) = &mut self.second else {
            return;
        };
        let all_mail = mailboxes
            .folders()
            .into_iter()
            .find(|folder| folder.name == ALL_MAIL)
            .unwrap();
        let copy = Message {
            header: format!(
                "From: {address}\r\nTo: {address}\r\nSubject: {DRAFT_SUBJECT}\r\nMessage-ID: {message_id}\r\n\r\n"
            ),
            ..Message::new(all_mail.uid_next)
        }
        .labeled(&[SENT_LABEL]);
        mailboxes.append(ALL_MAIL, copy);
    }

    /// How many messages of the Sent label carry the draft's subject,
    /// the account refreshed by a `/changes` call before each look and
    /// looked at again until one shows up or the looks run out.
    async fn sent_copies(&self, before: u64) -> usize {
        let mut copies = 0;
        for _ in 0..LOOKS {
            let _refreshed = self.changes("Email", before).await;
            let subjects = self.subjects_in("sent").await;
            copies = subjects
                .iter()
                .filter(|subject| *subject == DRAFT_SUBJECT)
                .count();
            if copies == 1 {
                break;
            }
            if matches!(self.second, Second::Account { .. }) {
                tokio::time::sleep(SETTLE).await;
            }
        }
        copies
    }

    /// The subjects of the mailbox with that role, newest first.
    async fn subjects_in(&self, role: &str) -> Vec<String> {
        let mailbox = self.suite.by_role(role);
        let ids = Self::listed(&self.query(&mailbox, LISTING, false).await, "ids");
        self.emails(&ids, &["subject"])
            .await
            .iter()
            .map(|email| email["subject"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}
