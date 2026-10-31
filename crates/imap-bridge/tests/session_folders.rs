// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session's folder commands against the scripted server in its
//! folder mode, its ACL mode and its Gmail mode: a typed name on the
//! wire, the folders under a renamed one, a delete the server refuses
//! or keeps as `\Noselect`, the subscriptions, the rights per mailbox
//! and through LIST plus the NO codes the parser drops.

mod session_rig;

use huliho_imap_bridge::session::{
    ImapSession, ListReturn, MailboxName, NameError, Place, Session, SessionError,
};
use huliho_imap_bridge::testing::imap::FakeImap;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, OWNER_RIGHTS};
use huliho_imap_bridge::testing::{
    Behavior, Extension, Folder, Mailboxes, Message, MyRights, ParentDelete,
};
use session_rig::{session as signed_in, start};

const TOP: Place<'static> = Place {
    parent: None,
    delimiter: Some('/'),
};

fn under(parent: &str) -> Place<'_> {
    Place {
        parent: Some(parent),
        delimiter: Some('/'),
    }
}

fn named(typed: &str, place: Place<'_>) -> MailboxName {
    MailboxName::new(typed, place).unwrap()
}

/// Every folder command the server received, without its tag.
fn sent(fake: &FakeImap) -> Vec<String> {
    let verbs = [
        "CREATE ",
        "RENAME ",
        "DELETE ",
        "SUBSCRIBE ",
        "UNSUBSCRIBE ",
        "MYRIGHTS ",
    ];
    fake.lines()
        .iter()
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command.to_owned())
        .filter(|command| verbs.iter().any(|verb| command.starts_with(verb)))
        .collect()
}

/// The folders of the server by name, with whether each is subscribed.
fn standing(fake: &FakeImap) -> Vec<(String, bool)> {
    let folders = fake.script().mailboxes.folders();
    folders
        .into_iter()
        .map(|folder| (folder.name, folder.subscribed))
        .collect()
}

/// The session still answers after a refusal: nothing was left unread.
async fn answers(session: &mut ImapSession) {
    session.noop().await.unwrap();
}

#[tokio::test]
async fn a_typed_name_is_created_in_modified_utf7_and_its_folders_follow_a_rename_rfc3501_6_3_5() {
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    let projects = named("Projecten ö", TOP);
    session.create(&projects).await.unwrap();
    session
        .create(&named("Klant \"A\"", under(projects.wire())))
        .await
        .unwrap();
    let renamed = named("Archief ü", TOP);
    session.rename(projects.wire(), &renamed).await.unwrap();
    let names: Vec<String> = standing(&fake).into_iter().map(|(name, _)| name).collect();
    assert!(names.contains(&"Archief &APw-".to_owned()), "{names:?}");
    assert!(
        names.contains(&"Archief &APw-/Klant \"A\"".to_owned()),
        "{names:?}"
    );
    assert!(!names.iter().any(|name| name.starts_with("Projecten")));
    assert_eq!(
        sent(&fake),
        [
            "CREATE \"Projecten &APY-\"",
            "CREATE \"Projecten &APY-/Klant \\\"A\\\"\"",
            "RENAME \"Projecten &APY-\" \"Archief &APw-\"",
        ]
    );
}

#[tokio::test]
async fn subscribe_and_unsubscribe_reach_the_folder_rfc3501_6_3_6_rfc3501_6_3_7() {
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    let work = named("Work", TOP);
    session.create(&work).await.unwrap();
    let subscribed = |fake: &FakeImap| standing(fake).into_iter().find(|(name, _)| name == "Work");
    assert_eq!(subscribed(&fake), Some(("Work".to_owned(), false)));
    session.subscribe("Work").await.unwrap();
    assert_eq!(subscribed(&fake), Some(("Work".to_owned(), true)));
    assert_eq!(session.lsub().await.unwrap().last().unwrap(), "Work");
    session.unsubscribe("Work").await.unwrap();
    assert_eq!(subscribed(&fake), Some(("Work".to_owned(), false)));
}

#[tokio::test]
async fn a_delete_with_folders_under_it_is_refused_or_kept_as_noselect_rfc9051_6_3_5() {
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    session.create(&named("Work", TOP)).await.unwrap();
    session.create(&named("Sub", under("Work"))).await.unwrap();
    let refused = session.delete("Work").await;
    assert!(matches!(refused, Err(SessionError::Refused)), "{refused:?}");
    answers(&mut session).await;
    session.delete("Work/Sub").await.unwrap();
    session.delete("Work").await.unwrap();
    assert!(
        !standing(&fake)
            .iter()
            .any(|(name, _)| name.starts_with("Work"))
    );

    let mut kept = Mailboxes::dovecot();
    kept.behavior = Behavior {
        parent_delete: ParentDelete::KeepsName,
        ..Behavior::default()
    };
    kept.push(Folder::new("Work").with_mail(vec![Message::new(1)]));
    kept.push(Folder::new("Work/Sub"));
    let fake = start(kept).await;
    let mut session = signed_in(&fake).await;
    session.delete("Work").await.unwrap();
    let listing = session.list(ListReturn::default()).await.unwrap();
    let work = listing.entries.iter().find(|entry| entry.name == "Work");
    assert!(work.unwrap().attributes.contains(&"\\NOSELECT".to_owned()));
    let folders = fake.script().mailboxes.folders();
    let work = folders.iter().find(|folder| folder.name == "Work").unwrap();
    assert_eq!(work.mail, Vec::<Message>::new());
}

#[tokio::test]
async fn a_no_carrying_a_code_the_parser_drops_reads_as_refused_rfc5530_rfc9051_7_1() {
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    // ALREADYEXISTS and NONEXISTENT as the model answers them.
    let exists = session.create(&named("Sent", TOP)).await;
    let missing = session.rename("Nowhere", &named("Elsewhere", TOP)).await;
    let gone = session.delete("Nowhere").await;
    for refused in [exists, missing, gone] {
        assert!(matches!(refused, Err(SessionError::Refused)), "{refused:?}");
        answers(&mut session).await;
    }
    for code in ["ALREADYEXISTS", "NONEXISTENT", "HASCHILDREN", "USEATTR"] {
        let mut mailboxes = Mailboxes::dovecot();
        mailboxes.behavior = Behavior {
            folder_refusal: Some(code),
            ..Behavior::default()
        };
        let fake = start(mailboxes).await;
        let mut session = signed_in(&fake).await;
        let answered = [
            session.create(&named("Work", TOP)).await,
            session.rename("Sent", &named("Outbox", TOP)).await,
            session.delete("Trash").await,
            session.subscribe("Junk").await,
            session.unsubscribe("Junk").await,
        ];
        for refused in answered {
            assert!(
                matches!(refused, Err(SessionError::Refused)),
                "{code} {refused:?}"
            );
        }
        answers(&mut session).await;
    }
}

#[tokio::test]
async fn a_no_naming_a_missing_mailbox_in_raw_utf8_fails_the_parse_as_on_dovecot() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.behavior = Behavior {
        utf8_no: true,
        ..Behavior::default()
    };
    let fake = start(mailboxes).await;
    let mut session = signed_in(&fake).await;
    let refused = session.delete("Caf&AOk-").await;
    assert!(
        matches!(refused, Err(SessionError::Protocol(_))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_name_that_cannot_name_a_mailbox_sends_nothing() {
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    for (typed, refused) in [
        ("Work\r\nA9 DELETE INBOX", NameError::Control),
        ("Work/Sub", NameError::Delimiter),
        (" Work", NameError::EdgeSpace),
        ("", NameError::Empty),
    ] {
        assert_eq!(MailboxName::new(typed, TOP), Err(refused), "{typed:?}");
    }
    let long = "a".repeat(huliho_imap_bridge::session::MAX_MAILBOX_NAME_BYTES + 1);
    assert_eq!(MailboxName::new(&long, TOP), Err(NameError::TooLong));
    // A stored wire name that holds a line break never goes out either.
    let refused = session.delete("Work\r\nA9 DELETE INBOX").await;
    assert!(
        matches!(refused, Err(SessionError::Protocol(_))),
        "{refused:?}"
    );
    answers(&mut session).await;
    assert!(sent(&fake).is_empty(), "{:?}", sent(&fake));
}

/// A Cyrus-shaped server: ACL with the rights it adds and, where asked,
/// LIST-MYRIGHTS; one folder the owner holds `lrs` on.
fn cyrus(list_rights: bool) -> Mailboxes {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.extensions.insert(Extension::Acl);
    if list_rights {
        mailboxes.extensions.insert(Extension::ListMyrights);
    }
    mailboxes.push(Folder {
        rights: MyRights::Letters("lrs"),
        ..Folder::new("Locked")
    });
    mailboxes.push(Folder::noselect("Shared"));
    mailboxes
}

#[tokio::test]
async fn myrights_names_the_rights_per_mailbox_and_through_list_rfc4314_3_5_rfc8440_3() {
    let fake = start(cyrus(true)).await;
    let mut session = signed_in(&fake).await;
    let acl = session.capabilities().await.unwrap().acl();
    assert!(acl.my_rights && acl.list_rights);
    let owner = session.my_rights("INBOX").await.unwrap();
    assert_eq!(owner.letters(), "acdeiklnprstwx");
    assert_eq!(owner.letters().len(), OWNER_RIGHTS.len());
    let locked = session.my_rights("Locked").await.unwrap();
    assert_eq!(locked.letters(), "lrs");
    assert!(!locked.has('k') && !locked.has('x') && !locked.has('t'));
    let missing = session.my_rights("Nowhere").await;
    assert!(matches!(missing, Err(SessionError::Refused)), "{missing:?}");
    let options = ListReturn {
        subscribed: true,
        my_rights: true,
        ..ListReturn::default()
    };
    let listing = session.list(options).await.unwrap();
    let rights: Vec<(&str, &str)> = listing
        .rights
        .iter()
        .map(|entry| (entry.mailbox.as_str(), entry.rights.letters()))
        .collect();
    assert!(rights.contains(&("INBOX", owner.letters())), "{rights:?}");
    assert!(rights.contains(&("Locked", "lrs")), "{rights:?}");
    // A mailbox nothing can select gets no line (RFC 8440 section 3).
    assert!(
        !rights.iter().any(|(name, _)| *name == "Shared"),
        "{rights:?}"
    );
    assert_eq!(rights.len(), listing.entries.len() - 1);
}

#[tokio::test]
async fn a_server_without_acl_or_list_myrights_answers_bad_and_the_session_stands() {
    let fake = start(cyrus(false)).await;
    let mut session = signed_in(&fake).await;
    let acl = session.capabilities().await.unwrap().acl();
    assert!(acl.my_rights && !acl.list_rights);
    let options = ListReturn {
        my_rights: true,
        ..ListReturn::default()
    };
    let listing = session.list(options).await;
    assert!(
        matches!(listing, Err(SessionError::Protocol(_))),
        "{listing:?}"
    );
    answers(&mut session).await;
    let fake = start(Mailboxes::dovecot()).await;
    let mut session = signed_in(&fake).await;
    assert!(!session.capabilities().await.unwrap().acl().my_rights);
    let unknown = session.my_rights("INBOX").await;
    assert!(
        matches!(unknown, Err(SessionError::Protocol(_))),
        "{unknown:?}"
    );
    answers(&mut session).await;
}

#[tokio::test]
async fn a_gmail_label_is_created_renamed_and_deleted_with_its_mail_kept_in_all_mail() {
    let mail = vec![Message::new(1).labeled(&["\\Inbox"]), Message::new(2)];
    let fake = start(Mailboxes::gmail(mail)).await;
    let mut session = signed_in(&fake).await;
    let label = named("Reizen ✈", TOP);
    session.create(&label).await.unwrap();
    fake.script()
        .mailboxes
        .relabel(ALL_MAIL, 1, &["\\Inbox", label.wire()]);
    let renamed = named("Vakantie", TOP);
    session.rename(label.wire(), &renamed).await.unwrap();
    let labels = |fake: &FakeImap| {
        let folders = fake.script().mailboxes.folders();
        let all = folders
            .iter()
            .find(|folder| folder.name == ALL_MAIL)
            .unwrap();
        all.mail
            .iter()
            .map(|message| message.labels.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&fake)[0], ["\\Inbox", "Vakantie"]);
    session.subscribe("Vakantie").await.unwrap();
    session.delete("Vakantie").await.unwrap();
    assert_eq!(labels(&fake), [vec!["\\Inbox"], vec!["\\Inbox"]]);
    assert!(!standing(&fake).iter().any(|(name, _)| name == "Vakantie"));
    // The stores and the folders Gmail keeps for itself stay.
    for system in [ALL_MAIL, "[Gmail]", "INBOX"] {
        let refused = session.delete(system).await;
        assert!(
            matches!(refused, Err(SessionError::Refused)),
            "{system} {refused:?}"
        );
    }
    let reserved = session.create(&named("Mine", under("[Gmail]"))).await;
    assert!(
        matches!(reserved, Err(SessionError::Refused)),
        "{reserved:?}"
    );
}
