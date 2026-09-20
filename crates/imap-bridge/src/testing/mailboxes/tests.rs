// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox model against the commands a pass sends.

use super::*;

#[test]
fn a_return_option_behind_an_absent_extension_is_bad_rfc5258() {
    let plain = Mailboxes::default();
    let bad = plain.answer("LIST", "LIST \"\" \"*\" RETURN (SUBSCRIBED)", "A1");
    assert_eq!(bad, "A1 BAD not offered\r\n");
    let extended = Mailboxes::new(
        vec![Folder::new("INBOX")],
        [Extension::ListExtended].into_iter().collect(),
    );
    let special = extended.answer("LIST", "LIST \"\" \"*\" RETURN (SPECIAL-USE)", "A1");
    assert_eq!(special, "A1 BAD not offered\r\n");
    let status = extended.answer("LIST", "LIST \"\" \"*\" RETURN (STATUS (MESSAGES))", "A1");
    assert_eq!(status, "A1 BAD not offered\r\n");
}

#[test]
fn a_dovecot_shaped_list_carries_attributes_status_and_the_tagged_ok() {
    let mailboxes = Mailboxes::dovecot();
    let answer = mailboxes.answer(
        "LIST",
        "LIST \"\" \"*\" RETURN (SUBSCRIBED SPECIAL-USE STATUS (MESSAGES UNSEEN))",
        "A2",
    );
    assert!(
        answer.starts_with("* LIST (\\HasNoChildren \\Subscribed) \"/\" \"INBOX\"\r\n* STATUS \"INBOX\" (MESSAGES 17 UNSEEN 3)\r\n"),
        "{answer}"
    );
    assert!(answer.contains("* LIST (\\HasNoChildren \\Subscribed \\Sent) \"/\" \"Sent\"\r\n"));
    assert!(answer.ends_with("A2 OK done\r\n"));
}

#[test]
fn status_answers_no_for_a_missing_mailbox_and_quotes_specials() {
    let mailboxes = Mailboxes::new(vec![Folder::new("Say \"hi\"")], BTreeSet::new());
    let found = mailboxes.answer("STATUS", "STATUS \"Say \\\"hi\\\"\" (MESSAGES)", "A3");
    assert_eq!(
        found,
        "* STATUS \"Say \\\"hi\\\"\" (MESSAGES 0)\r\nA3 OK done\r\n"
    );
    let missing = mailboxes.answer("STATUS", "STATUS \"Other\" (MESSAGES)", "A4");
    assert_eq!(missing, "A4 NO refused\r\n");
    let modseq = mailboxes.answer("STATUS", "STATUS \"Say \\\"hi\\\"\" (HIGHESTMODSEQ)", "A5");
    assert_eq!(modseq, "A5 BAD not offered\r\n");
}

#[test]
fn a_folder_that_refuses_status_is_listed_as_selectable_and_answers_no() {
    let shared = Folder {
        refuses_status: true,
        ..Folder::new("Shared")
    };
    let mailboxes = Mailboxes::new(vec![shared], BTreeSet::new());
    let listed = mailboxes.answer("LIST", "LIST \"\" \"*\"", "A6");
    assert_eq!(
        listed,
        "* LIST (\\HasNoChildren) \"/\" \"Shared\"\r\nA6 OK done\r\n"
    );
    let refused = mailboxes.answer("STATUS", "STATUS \"Shared\" (MESSAGES)", "A7");
    assert_eq!(refused, "A7 NO refused\r\n");
}
