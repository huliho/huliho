// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The folder commands against the compose targets: a folder with a
//! name outside ASCII created, renamed with its mail and a folder under
//! it, subscribed, unsubscribed and deleted on Dovecot, each step seen
//! from a second connection and through the mailbox pass. Beside it the
//! rights Cyrus names over IMAPS, per mailbox and through LIST.

#![cfg(feature = "live-targets")]

mod live_rig;

use async_imap::imap_proto::{MailboxDatum, NameAttribute, Response, Status};
use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::session::{ListReturn, MailboxName, Place, Rights, Session, SessionError};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::utf7;
use live_rig::{
    Editor, USER, append, bridge_session, bridge_session_on, cache, editor, editor_on, link,
    mailbox_list, within,
};

/// The names of the cycle, each outside ASCII, so the encoder runs
/// live.
const FOLDER: &str = "Mappen ü";
const RENAMED: &str = "Mappen ö";
const CHILD: &str = "Kind é";

/// The compose Cyrus over implicit TLS.
const CYRUS_IMAPS_PORT: u16 = 9993;

/// One name as the second connection lists it.
#[derive(Debug)]
struct Seen {
    name: String,
    selectable: bool,
    subscribed: bool,
}

/// The names a LIST or LSUB of the second connection answers, as the
/// wire spells them, each with whether it can be selected.
async fn listed(editor: &mut Editor, command: &str) -> Vec<(String, bool)> {
    let id = within(editor.run_command(command)).await.unwrap();
    let mut names = Vec::new();
    loop {
        let response = within(editor.read_response()).await.unwrap().unwrap();
        match response.parsed() {
            Response::MailboxData(MailboxDatum::List {
                name,
                name_attributes,
                ..
            }) => {
                let noselect = name_attributes.contains(&NameAttribute::NoSelect);
                names.push((name.to_string(), !noselect));
            }
            Response::Done { tag, status, .. } if *tag == id => {
                assert_eq!(*status, Status::Ok, "{command}");
                return names;
            }
            _ => {}
        }
    }
}

/// Every name the second connection lists.
async fn seen(editor: &mut Editor) -> Vec<Seen> {
    let subscribed = listed(editor, "LSUB \"\" \"*\"").await;
    let listed = listed(editor, "LIST \"\" \"*\"").await;
    listed
        .into_iter()
        .map(|(name, selectable)| Seen {
            subscribed: subscribed.iter().any(|(held, _)| *held == name),
            name,
            selectable,
        })
        .collect()
}

fn find<'a>(seen: &'a [Seen], name: &str) -> Option<&'a Seen> {
    seen.iter().find(|found| found.name == name)
}

/// Removes what an earlier run left, children first, each step on a
/// connection of its own: Dovecot's NO for a missing name holds UTF-8,
/// which the parser refuses.
async fn cleared(names: &[&str]) {
    for name in names {
        let mut second = editor().await;
        let _ = within(second.unsubscribe(name)).await;
        let mut second = editor().await;
        if within(second.delete(name)).await.is_ok() {
            within(second.logout()).await.unwrap();
        }
    }
}

/// The names the bridge's own `Mailbox/get` shows after a mailbox pass.
async fn passed(session: &mut impl Session, cache: &Cache) -> Vec<String> {
    sync(session, cache).await.unwrap();
    let list = mailbox_list(cache, &link()).await;
    list.iter()
        .map(|mailbox| mailbox["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn dovecot_runs_the_folder_cycle_a_second_client_sees_rfc3501_6_3_3_to_6_3_7() {
    let mut session = bridge_session().await;
    let listing = session.list(ListReturn::default()).await.unwrap();
    let inbox = listing.entries.iter().find(|entry| entry.name == "INBOX");
    let delimiter = inbox.unwrap().delimiter;
    let top = Place {
        parent: None,
        delimiter,
    };
    let folder = MailboxName::new(FOLDER, top).unwrap();
    let renamed = MailboxName::new(RENAMED, top).unwrap();
    let under = |parent: &MailboxName| {
        let place = Place {
            parent: Some(parent.wire()),
            delimiter,
        };
        MailboxName::new(CHILD, place).unwrap()
    };
    let (child, moved) = (under(&folder), under(&renamed));
    cleared(&[child.wire(), folder.wire(), moved.wire(), renamed.wire()]).await;
    let cache = cache("live-folders");
    let mut editor = editor().await;

    session.create(&folder).await.unwrap();
    session.create(&child).await.unwrap();
    let message = format!("From: {USER}\r\nTo: {USER}\r\nSubject: folders\r\n\r\nBody\r\n");
    append(&mut editor, folder.wire(), None, &message).await;
    let now = seen(&mut editor).await;
    assert!(find(&now, folder.wire()).is_some_and(|found| found.selectable));
    assert!(find(&now, child.wire()).is_some(), "{now:?}");
    let names = passed(&mut session, &cache).await;
    assert!(names.contains(&FOLDER.to_owned()) && names.contains(&CHILD.to_owned()));

    // The mail and the folder under it go along (RFC 3501 section 6.3.5).
    session.rename(folder.wire(), &renamed).await.unwrap();
    let now = seen(&mut editor).await;
    assert!(find(&now, folder.wire()).is_none(), "{now:?}");
    assert!(find(&now, moved.wire()).is_some(), "{now:?}");
    let status = within(editor.status(renamed.wire(), "(MESSAGES)"))
        .await
        .unwrap();
    assert_eq!(status.exists, 1);
    let names = passed(&mut session, &cache).await;
    assert!(names.contains(&RENAMED.to_owned()) && !names.contains(&FOLDER.to_owned()));
    assert_eq!(utf7::decode(renamed.wire()), RENAMED);

    for subscribed in [true, false] {
        if subscribed {
            session.subscribe(renamed.wire()).await.unwrap();
        } else {
            session.unsubscribe(renamed.wire()).await.unwrap();
        }
        let now = seen(&mut editor).await;
        let found = find(&now, renamed.wire()).map(|found| found.subscribed);
        assert_eq!(found, Some(subscribed), "{now:?}");
    }

    // This Dovecot keeps the name of a deleted folder that has one under
    // it as \Noselect without its mail (RFC 3501 section 6.3.4) and drops
    // it with the last one under it.
    session.delete(renamed.wire()).await.unwrap();
    let now = seen(&mut editor).await;
    assert!(find(&now, renamed.wire()).is_some_and(|found| !found.selectable));
    assert!(find(&now, moved.wire()).is_some_and(|found| found.selectable));
    session.delete(moved.wire()).await.unwrap();
    let now = seen(&mut editor).await;
    assert!(
        !now.iter().any(|found| found.name.starts_with("Mappen")),
        "{now:?}"
    );
    let names = passed(&mut session, &cache).await;
    assert!(
        !names.iter().any(|name| name.starts_with("Mappen")),
        "{names:?}"
    );

    // No ACL here, so MYRIGHTS is a command Dovecot does not know.
    assert!(!session.capabilities().await.unwrap().acl().my_rights);
    let unknown = session.my_rights("INBOX").await;
    assert!(
        matches!(unknown, Err(SessionError::Protocol(_))),
        "{unknown:?}"
    );
    session.noop().await.unwrap();
    session.logout().await.unwrap();
    within(editor.logout()).await.unwrap();
}

/// What the second connection's own MYRIGHTS answers, in the form the
/// bridge keeps.
async fn rights_by_hand(editor: &mut Editor, mailbox: &str) -> String {
    let id = within(editor.run_command(format!("MYRIGHTS \"{mailbox}\"")))
        .await
        .unwrap();
    let mut letters = None;
    loop {
        let response = within(editor.read_response()).await.unwrap().unwrap();
        match response.parsed() {
            Response::MyRights(found) => {
                let mut held: Vec<char> = found
                    .rights
                    .iter()
                    .map(|right| char::from(*right))
                    .collect();
                held.sort_unstable();
                held.dedup();
                letters = Some(held.into_iter().collect());
            }
            Response::Done { tag, .. } if *tag == id => return letters.unwrap(),
            _ => {}
        }
    }
}

#[tokio::test]
async fn cyrus_names_the_rights_per_mailbox_and_through_list_as_a_second_client_reads_them_rfc4314_3_5_rfc8440_3()
 {
    let mut session = bridge_session_on(CYRUS_IMAPS_PORT).await;
    let capabilities = session.capabilities().await.unwrap();
    let acl = capabilities.acl();
    assert!(acl.my_rights && acl.list_rights);
    // RIGHTS= names the rights a server adds to those of RFC 2086 (RFC
    // 4314 section 5.1.1).
    let extra = capabilities
        .iter()
        .find_map(|name| name.strip_prefix("RIGHTS="))
        .unwrap()
        .to_ascii_lowercase();
    let inbox = session.my_rights("INBOX").await.unwrap();
    assert!(
        extra.chars().all(|letter| inbox.has(letter)),
        "{extra} {inbox:?}"
    );
    let mut editor = editor_on(CYRUS_IMAPS_PORT).await;
    assert_eq!(inbox.letters(), rights_by_hand(&mut editor, "INBOX").await);
    let options = ListReturn {
        subscribed: true,
        my_rights: true,
        ..ListReturn::default()
    };
    let listing = session.list(options).await.unwrap();
    assert_eq!(listing.rights.len(), listing.entries.len(), "{listing:?}");
    for entry in &listing.rights {
        let one: Rights = session.my_rights(&entry.mailbox).await.unwrap();
        assert_eq!(entry.rights, one, "{}", entry.mailbox);
        let own = rights_by_hand(&mut editor, &entry.mailbox).await;
        assert_eq!(entry.rights.letters(), own, "{}", entry.mailbox);
    }
    session.logout().await.unwrap();
    within(editor.logout()).await.unwrap();
}
