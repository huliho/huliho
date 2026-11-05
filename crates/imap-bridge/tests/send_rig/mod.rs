// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the submission tests share: a synced account with a Drafts and a
//! Sent folder, a second link whose sends reach the scripted SMTP server,
//! the calls under the submission capability and what each server took.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY, handle};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::testing::smtp::{self, FakeSmtp, MESSAGE};
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Submitting, TestConnector, imap};
use serde_json::{Value, json};

use crate::draft_rig::{self, DRAFTS, create, created};
use crate::set_rig::Set;
use crate::sync_rig::ACCOUNT;

pub const SENT: &str = "Sent";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// The account, the SMTP server and the link its sends go through with
/// the connector behind it.
pub struct Sending {
    pub set: Set,
    pub smtp: FakeSmtp,
    pub connector: Submitting,
    pub link: Arc<Link<Submitting>>,
}

/// INBOX, Drafts, Sent and Trash behind the extensions given.
pub fn account(extensions: BTreeSet<Extension>) -> Mailboxes {
    let mailboxes = draft_rig::account(extensions);
    mailboxes.push(Folder::special(SENT, "\\Sent"));
    mailboxes
}

/// The account over the model with its Drafts and Sent synced, beside
/// an SMTP server running the script.
pub async fn sending(mailboxes: Mailboxes, script: smtp::Script) -> Sending {
    let set = Box::pin(draft_rig::synced(mailboxes)).await;
    Box::pin(set.rig.sync(SENT)).await;
    over(set, script).await
}

/// The account as the set left it, beside an SMTP server running the
/// script.
pub async fn over(set: Set, script: smtp::Script) -> Sending {
    let smtp = FakeSmtp::start(script).await;
    let fake = &set.rig.fake;
    let inner = TestConnector::scripted(
        fake.trusting(),
        fake.target(imap::HOST, TlsMode::Implicit),
        STEP,
    );
    let connector = Submitting {
        inner,
        tls: smtp.trusting(),
        smtp: smtp.target(smtp::HOST, TlsMode::Implicit),
        step: STEP,
    };
    Sending {
        set,
        smtp,
        link: Arc::new(Link::with_interval(connector.clone(), Duration::ZERO)),
        connector,
    }
}

impl Sending {
    /// A draft saved through `Email/set`; its id.
    pub async fn draft(&self, object: Value) -> String {
        created(&create(&self.set, object).await)
    }

    /// The id of a folder by its wire name.
    pub fn mailbox(&self, name: &str) -> String {
        self.set.rig.folder(name).id.to_string()
    }

    /// The patch a client files a sent draft with: out of Drafts, into
    /// Sent, its draft keyword cleared.
    pub fn filing(&self) -> Value {
        json!({
            (format!("mailboxIds/{}", self.mailbox(DRAFTS))): null,
            (format!("mailboxIds/{}", self.mailbox(SENT))): true,
            "keywords/$draft": null,
        })
    }

    /// The Response object to these calls under the submission
    /// capability.
    pub async fn calls(&self, calls: &[Value]) -> Value {
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY],
            "methodCalls": calls,
        });
        let bytes = handle(
            &self.set.rig.cache,
            &self.link,
            &serde_json::to_vec(&body).unwrap(),
            ACCOUNT,
        )
        .await
        .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// `EmailSubmission/set` with one create `s` of the email, the
    /// arguments given beside it; the Response object.
    pub async fn submit(&self, email: &str, mut arguments: Value) -> Value {
        arguments["accountId"] = json!(ACCOUNT);
        arguments["create"]["s"]["identityId"] = json!(ACCOUNT);
        arguments["create"]["s"]["emailId"] = json!(email);
        self.calls(&[json!(["EmailSubmission/set", arguments, "c1"])])
            .await
    }

    /// The messages DATA brought, in order.
    pub fn messages(&self) -> Vec<String> {
        let lines = self.smtp.lines();
        let prefix = format!("tls {MESSAGE}");
        let found = lines.iter().filter_map(|line| line.strip_prefix(&prefix));
        found.map(str::to_owned).collect()
    }

    /// The commands of the transaction the server received.
    pub fn transaction(&self) -> Vec<String> {
        let lines = self.smtp.lines();
        let found = lines.iter().filter_map(|line| line.strip_prefix("tls "));
        let verbs = ["MAIL", "RCPT", "RSET", "DATA"];
        found
            .filter(|line| verbs.iter().any(|verb| line.starts_with(verb)))
            .map(str::to_owned)
            .collect()
    }
}

/// The answer to the submission and the implicit `Email/set`, where one
/// came.
pub fn answers(response: &Value) -> (&Value, Option<&Value>) {
    let responses = response["methodResponses"].as_array().unwrap();
    assert_eq!(responses[0][0], "EmailSubmission/set", "{response}");
    let implicit = responses.get(1).map(|implicit| {
        assert_eq!(implicit[0], "Email/set", "{response}");
        assert_eq!(implicit[2], responses[0][2], "{response}");
        &implicit[1]
    });
    (&responses[0][1], implicit)
}

/// The type of the error the create `s` ended with.
pub fn refusal(response: &Value) -> &str {
    let (set, _) = answers(response);
    let kind = set["notCreated"]["s"]["type"].as_str();
    kind.unwrap_or_else(|| panic!("{response}"))
}
