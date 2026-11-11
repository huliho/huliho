// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The draft cycle through `Email/set` (RFC 8621 section 4.6), each step
//! seen from the second connection: a draft with a subject outside
//! ASCII, two recipients, a text and an HTML part, the attachment of a
//! message the account holds and a file sent through the upload route
//! is created and read the way another client reads it, replaced with a
//! new id, then destroyed. A target that lists no mailbox with the role
//! a step needs gets a folder of the run, removed after.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use huliho_imap_bridge::utf7;
use mail_parser::{MessageParser, MimeHeaders};
use serde_json::{Value, json};

use super::corpus::{RICH_NOTES, RICH_NOTES_NAME};
use super::reading::attachment;
use super::{
    Editor, Fetched, LISTING, Live, MAIL_ADDRESS, Mail, Target, Window, clear, editor,
    expunge_subject, fetched, leave, listed, number_of, seed_rich, with_subject,
};
use crate::answers::answer;
use crate::signin::with_cookie;

/// The corpus number of the rich message whose attachment the draft
/// carries.
const RICH: u32 = 1;

/// The word every subject of the cycle starts with, by which a run
/// finds what an earlier one left.
const WORD: &str = "Conceptronde";

/// What every folder of a run is named with, before its role and stamp.
const FOLDER_PREFIX: &str = "Compose ";

/// The two recipients, one with a display name.
const RECIPIENTS: [(&str, &str); 2] =
    [("Anna Jansen", "anna@huliho.test"), ("", "mo@live.example")];

const TEXT: &str = "Hallo Anna, de offerte zit erbij.";
const HTML: &str = "<p>Hallo Anna, de <b>offerte</b> zit erbij.</p>";

/// The file sent through the upload route, its name outside ASCII.
const UPLOAD_NAME: &str = "Offerte één.txt";
const UPLOAD_TYPE: &str = "text/plain";
const UPLOAD_TEXT: &str = "Twee badkamers en één keuken.";

/// How long the account may take to list its mailboxes or sync its inbox.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// A mark no earlier run used, which the second connection searches by.
pub fn stamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    format!("r{}", now.unwrap_or_default().as_millis())
}

/// A mailbox a step files into: its id, its name on the wire and
/// whether this run made it.
pub struct Folder {
    pub id: String,
    pub wire: String,
    made: bool,
}

impl Folder {
    /// The account's top-level mailbox with that role, else a folder of
    /// the run.
    pub async fn of(live: &Live, mail: &Mail, role: &str, stamp: &str) -> Self {
        let mailboxes = live.mailboxes(mail).await;
        if let Some(found) = mailboxes.iter().find(|mailbox| mailbox["role"] == role) {
            assert!(found["parentId"].is_null(), "{found}");
            return Self {
                id: found["id"].as_str().unwrap().to_owned(),
                wire: utf7::encode(found["name"].as_str().unwrap()),
                made: false,
            };
        }
        let name = format!("{FOLDER_PREFIX}{role} {stamp}");
        let create = json!({ "f": { "name": name, "parentId": null } });
        let arguments = json!({ "accountId": mail.account, "create": create });
        let made = live.call(mail, "Mailbox/set", arguments).await;
        let id = made["created"]["f"]["id"].as_str();
        Self {
            id: id.unwrap_or_else(|| panic!("{made}")).to_owned(),
            wire: name,
            made: true,
        }
    }

    /// A folder this run made goes with what it holds.
    pub async fn leave(self, live: &Live, mail: &Mail) {
        if self.made {
            removed(live, mail, vec![self.id]).await;
        }
    }
}

/// The folders gone, with every email they hold (RFC 8621 section 2.5),
/// in whatever order the server names them.
async fn removed(live: &Live, mail: &Mail, mut ids: Vec<String>) {
    let arguments = json!({
        "accountId": mail.account,
        "destroy": ids,
        "onDestroyRemoveEmails": true,
    });
    let gone = live.call(mail, "Mailbox/set", arguments).await;
    let mut destroyed = listed(&gone, "destroyed");
    destroyed.sort_unstable();
    ids.sort_unstable();
    assert_eq!(destroyed, ids, "{gone}");
}

/// Removes the folders an earlier run left behind.
pub async fn swept(live: &Live, mail: &Mail) {
    let left: Vec<String> = live
        .mailboxes(mail)
        .await
        .iter()
        .filter(|mailbox| {
            mailbox["name"]
                .as_str()
                .is_some_and(|name| name.starts_with(FOLDER_PREFIX))
        })
        .map(|mailbox| mailbox["id"].as_str().unwrap().to_owned())
        .collect();
    if !left.is_empty() {
        removed(live, mail, left).await;
    }
}

/// The cycle on one target.
pub async fn cycle(target: Target) {
    let live = Live::start().await;
    let mut editor = editor(target).await;
    clear(&mut editor).await;
    seed_rich(&mut editor, RICH).await;
    let mail = live.add(&target.body(), target.is_bridge()).await;
    let inbox = live.wait_synced(&mail, "inbox", PATIENCE).await;
    swept(&live, &mail).await;
    let stamp = stamp();
    let drafts = Folder::of(&live, &mail, "drafts", &stamp).await;
    expunge_subject(&mut editor, &drafts.wire, WORD).await;
    let kept = kept(&live, &mail, inbox["id"].as_str().unwrap()).await;
    let parts = (kept, uploaded(&live, &mail).await);
    let cycle = Cycle {
        live: &live,
        mail: &mail,
        drafts: &drafts,
        stamp: &stamp,
        parts: &parts,
    };
    let first = cycle.saved(&mut editor, "één", None).await;
    let second = cycle.saved(&mut editor, "twee", Some(&first)).await;
    assert_ne!(second["id"], first["id"]);
    assert_ne!(second["blobId"], first["blobId"]);
    let arguments = json!({ "accountId": mail.account, "destroy": [second["id"]] });
    let gone = live.call(&mail, "Email/set", arguments).await;
    assert_eq!(gone["destroyed"], json!([second["id"]]), "{gone}");
    let left = with_subject(&mut editor, &drafts.wire, &stamp).await;
    assert_eq!(left, Vec::<u32>::new(), "the destroy leaves nothing");
    drafts.leave(&live, &mail).await;
    leave(&live, &mail, editor).await;
}

/// What every save of one cycle shares: the account, the mailbox, the
/// run's stamp and the two attachments.
struct Cycle<'a> {
    live: &'a Live,
    mail: &'a Mail,
    drafts: &'a Folder,
    stamp: &'a str,
    parts: &'a (Value, String),
}

impl Cycle<'_> {
    /// A draft under a subject with that word saved, the one it replaces
    /// destroyed beside it, then read back by the second connection and
    /// through the account: what `created` answers.
    async fn saved(&self, editor: &mut Editor, word: &str, replacing: Option<&Value>) -> Value {
        let subject = format!("{WORD} {word} {}", self.stamp);
        let object = draft(&self.drafts.id, &subject, self.parts);
        let created = saved(self.live, self.mail, object, replacing).await;
        let uids = with_subject(editor, &self.drafts.wire, self.stamp).await;
        assert_eq!(uids.len(), 1, "one draft under the stamp: {uids:?}");
        let read = fetched(editor, uids[0]).await;
        reads_as(&read, &subject);
        assert_eq!(created["size"], read.raw.len(), "the size is the server's");
        let id = created["id"].as_str().unwrap().to_owned();
        let properties = [
            "subject",
            "keywords",
            "mailboxIds",
            "hasAttachment",
            "threadId",
        ];
        let email = self.live.emails(self.mail, &[id], &properties).await;
        let email = &email[0];
        assert_eq!(email["subject"], subject, "{email}");
        assert_eq!(email["keywords"]["$draft"], true, "{email}");
        assert_eq!(email["mailboxIds"][&self.drafts.id], true, "{email}");
        assert_eq!(email["hasAttachment"], true, "{email}");
        assert_eq!(email["threadId"], created["threadId"], "{email}");
        created
    }
}

/// The attachment of the rich message, as the account lists it.
async fn kept(live: &Live, mail: &Mail, inbox: &str) -> Value {
    let window = Window {
        mailbox: inbox,
        position: 0,
        limit: LISTING,
        collapse: false,
        total: false,
    };
    let ids = listed(&live.query(mail, window).await, "ids");
    let found = live.emails(mail, &ids, &["id", "messageId"]).await;
    let rich = found.iter().find(|email| number_of(email) == Some(RICH));
    let id = rich.unwrap_or_else(|| panic!("{found:?}"))["id"].clone();
    let id = id.as_str().unwrap().to_owned();
    let email = live.emails(mail, &[id], &["attachments"]).await.remove(0);
    attachment(&email, RICH_NOTES_NAME).clone()
}

/// A file through the upload route (RFC 8620 section 6.1): its blob id.
async fn uploaded(live: &Live, mail: &Mail) -> String {
    let uri = format!("/api/jmap/{}/upload/{}", mail.id, mail.account);
    let mut request = with_cookie(Method::POST, &uri, &live.cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, UPLOAD_TYPE.parse().unwrap());
    *request.body_mut() = Body::from(UPLOAD_TEXT);
    let (status, answered) = answer(&live.router, request).await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    assert_eq!(answered["accountId"], mail.account.as_str(), "{answered}");
    assert_eq!(answered["size"], UPLOAD_TEXT.len(), "{answered}");
    let kind = answered["type"].as_str().unwrap_or_default();
    assert!(kind.starts_with(UPLOAD_TYPE), "{answered}");
    answered["blobId"].as_str().unwrap().to_owned()
}

/// The draft as a client sends it: a mixed part over the alternative of
/// text and HTML and the two attachments, each by its blob id.
fn draft(drafts: &str, subject: &str, (kept, upload): &(Value, String)) -> Value {
    let to: Vec<Value> = RECIPIENTS
        .iter()
        .map(|(name, email)| json!({ "name": name, "email": email }))
        .collect();
    let alternative = json!({
        "type": "multipart/alternative",
        "subParts": [
            { "type": "text/plain", "partId": "text" },
            { "type": "text/html", "partId": "html" },
        ],
    });
    let forwarded = json!({
        "type": kept["type"],
        "blobId": kept["blobId"],
        "name": RICH_NOTES_NAME,
        "disposition": "attachment",
    });
    let file = json!({
        "type": UPLOAD_TYPE,
        "blobId": upload,
        "name": UPLOAD_NAME,
        "disposition": "attachment",
    });
    json!({
        "mailboxIds": { drafts: true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "email": MAIL_ADDRESS }],
        "to": to,
        "subject": subject,
        "bodyStructure": { "type": "multipart/mixed", "subParts": [alternative, forwarded, file] },
        "bodyValues": { "text": { "value": TEXT }, "html": { "value": HTML } },
    })
}

/// One `Email/set` create, the draft it replaces destroyed beside it:
/// what `created` answers.
async fn saved(live: &Live, mail: &Mail, object: Value, replacing: Option<&Value>) -> Value {
    let mut arguments = json!({ "accountId": mail.account, "create": { "d": object } });
    if let Some(old) = replacing {
        arguments["destroy"] = json!([old["id"]]);
    }
    let answered = live.call(mail, "Email/set", arguments).await;
    assert_eq!(answered["notCreated"], Value::Null, "{answered}");
    assert_eq!(answered["notDestroyed"], Value::Null, "{answered}");
    if let Some(old) = replacing {
        assert_eq!(answered["destroyed"], json!([old["id"]]), "{answered}");
        let id = old["id"].as_str().unwrap().to_owned();
        let arguments = json!({ "accountId": mail.account, "ids": [id], "properties": ["id"] });
        let gone = live.call(mail, "Email/get", arguments).await;
        assert_eq!(gone["notFound"], json!([id]), "{gone}");
    }
    let created = answered["created"]["d"].clone();
    for property in ["id", "blobId", "threadId", "size"] {
        assert!(!created[property].is_null(), "{answered}");
    }
    created
}

/// What another client's parser reads in the draft: the flags, the
/// subject, the sender, both recipients, both alternatives and both
/// attachments with their names and bytes.
fn reads_as(read: &Fetched, subject: &str) {
    assert!(
        read.draft && read.seen,
        "a saved draft is flagged draft and seen"
    );
    let message = MessageParser::default().parse(&read.raw).unwrap();
    assert_eq!(message.subject(), Some(subject));
    let from = message.from().and_then(|from| from.first());
    assert_eq!(from.and_then(|sender| sender.address()), Some(MAIL_ADDRESS));
    let to: Vec<&str> = message
        .to()
        .unwrap()
        .iter()
        .filter_map(|recipient| recipient.address())
        .collect();
    let named: Vec<&str> = RECIPIENTS.iter().map(|(_, email)| *email).collect();
    assert_eq!(to, named);
    assert_eq!(message.body_text(0).unwrap().trim_end(), TEXT);
    assert!(message.body_html(0).unwrap().contains(HTML));
    let attached: Vec<(&str, String)> = message
        .attachments()
        .map(|part| {
            let bytes = String::from_utf8_lossy(part.contents());
            (
                part.attachment_name().unwrap_or_default(),
                bytes.trim_end().to_owned(),
            )
        })
        .collect();
    let expected = [
        (RICH_NOTES_NAME, RICH_NOTES.to_owned()),
        (UPLOAD_NAME, UPLOAD_TEXT.to_owned()),
    ];
    assert_eq!(attached, expected);
}
