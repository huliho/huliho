// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The steps of a message that is opened: its body with the attachment
//! listed, the attachment and the whole message as blobs, then the mark
//! as read, which a second client sees as the flag.

use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::record::fixed::fill;
use serde_json::{Value, json};

use super::super::corpus::{ATTACHED, ATTACHMENT, ATTACHMENT_NAME, SEEDED, seed};
use super::super::{ACCOUNT, gmail};
use super::{LOOKS, Scenario, Second};

/// The cap a client asks a body value under.
const BODY_VALUE_BYTES: u64 = 4 * 1024 * 1024;

impl Scenario {
    /// The message with an attachment answers its text as a value and
    /// its attachment as a part with a blob id.
    pub(super) async fn body(&mut self) {
        let id = self.ids[&ATTACHED].clone();
        let arguments = json!({
            "accountId": ACCOUNT,
            "ids": [id],
            "properties": ["textBody", "htmlBody", "attachments", "bodyValues"],
            "fetchTextBodyValues": true,
            "maxBodyValueBytes": BODY_VALUE_BYTES,
        });
        let opened = self.call("Email/get", arguments).await;
        let email = &opened["list"][0];
        let part = email["textBody"][0]["partId"].as_str().unwrap();
        let value = &email["bodyValues"][part];
        assert_eq!(value["isTruncated"], false, "{opened}");
        self.expect_bytes(value["value"].as_str().unwrap(), &seed(ATTACHED).body);
        let attachments = email["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 1, "{opened}");
        assert_eq!(attachments[0]["name"], ATTACHMENT_NAME);
        assert_eq!(attachments[0]["type"], "text/plain");
        assert_eq!(attachments[0]["blobId"], format!("{id}-2"));
    }

    /// The attachment and the whole message come as blobs.
    pub(super) async fn download(&mut self) {
        let id = self.ids[&ATTACHED].clone();
        let attachment = self.suite.blob(&format!("{id}-2")).await;
        self.expect_bytes(&attachment, ATTACHMENT);
        let whole = self.suite.blob(&id).await;
        if self.suite.is_replay() {
            assert_eq!(whole, fill(whole.len()));
        } else {
            assert!(whole.contains(&seed(ATTACHED).message_id()), "{whole}");
        }
    }

    /// The bytes of a part as this run sees them: what was sent, or
    /// filler of what the server answered behind a transcript.
    fn expect_bytes(&self, got: &str, sent: &str) {
        if self.suite.is_replay() {
            assert_eq!(got, fill(got.len()));
        } else {
            assert_eq!(got.trim_end(), sent);
        }
    }

    /// The unread emails of the inbox as `Mailbox/get` counts them.
    async fn unread(&self) -> u64 {
        let inbox = self.suite.by_role("inbox").id;
        let arguments = json!({ "accountId": ACCOUNT, "ids": [inbox] });
        let got = self.call("Mailbox/get", arguments).await;
        got["list"][0]["unreadEmails"].as_u64().unwrap()
    }

    /// Whether the second client sees the message as read.
    async fn second_sees_read(&mut self, number: u32) -> bool {
        match &mut self.second {
            Second::Nobody => true,
            Second::Model(mailboxes) => mailboxes
                .folders()
                .iter()
                .filter(|folder| folder.name == ALL_MAIL)
                .flat_map(|folder| &folder.mail)
                .filter(|message| message.uid == number)
                .any(|message| message.flags.iter().any(|flag| flag == "\\Seen")),
            Second::Account { editor, stores } => {
                let mut seen = gmail::seen(editor, stores, number).await;
                for _ in 1..LOOKS {
                    if seen {
                        break;
                    }
                    tokio::time::sleep(gmail::SETTLE).await;
                    seen = gmail::seen(editor, stores, number).await;
                }
                seen
            }
        }
    }

    /// The unseen message is marked read: the email and its mailboxes
    /// read as updated, the count follows and the flag stands on the
    /// server.
    pub(super) async fn mark_read(&mut self) {
        let id = self.ids[&SEEDED].clone();
        let unread = self.unread().await;
        let before = self.suite.state();
        let arguments = json!({
            "accountId": ACCOUNT,
            "update": { &id: { "keywords/$seen": true } },
        });
        let stored = self.call("Email/set", arguments).await;
        assert_eq!(stored["updated"], json!({ &id: null }), "{stored}");
        assert_eq!(stored["notUpdated"], Value::Null);
        let read = self.email(SEEDED, &["keywords"]).await;
        assert_eq!(read["keywords"]["$seen"], true);
        assert_eq!(self.unread().await, unread - 1);
        let changed = self.changes("Email", before).await;
        assert!(Self::names(&changed, "updated", &id), "{changed}");
        let mailboxes = Self::listed(&self.changes("Mailbox", before).await, "updated");
        for role in ["inbox", "archive"] {
            let mailbox = self.suite.by_role(role).id.to_string();
            assert!(mailboxes.contains(&mailbox), "{role}: {mailboxes:?}");
        }
        assert!(self.second_sees_read(SEEDED).await);
    }
}
