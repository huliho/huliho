// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session's quota root against the scripted server: the storage in
//! octets and the message count of a Dovecot-shaped root and of Gmail's,
//! an inbox under no root and the QUOTA lines a server could send to
//! hurt, each read under the guard and the bounds of RFC 9208.

mod session_rig;

use huliho_imap_bridge::session::{MAX_WIRE_NAME_BYTES, QuotaRoot, Session, SessionError, Usage};
use huliho_imap_bridge::testing::mailboxes::quota_lines;
use huliho_imap_bridge::testing::{Extension, Mailboxes};
use session_rig::{session, start};

/// The octets in one unit of STORAGE (RFC 9208 section 5.1).
const KIB: u64 = 1024;

/// A Dovecot-shaped server with QUOTA on and these quota lines.
fn quota(lines: Option<String>) -> Mailboxes {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.extensions.insert(Extension::Quota);
    mailboxes.quota = lines;
    mailboxes
}

/// What the quota root of the inbox reads as on that server.
async fn read(mailboxes: Mailboxes) -> Result<Vec<QuotaRoot>, SessionError> {
    let fake = start(mailboxes).await;
    let roots = session(&fake).await.quota_root().await;
    let asked = fake.lines();
    assert!(
        asked
            .iter()
            .any(|line| line.ends_with(" GETQUOTAROOT INBOX"))
    );
    roots
}

#[tokio::test]
async fn a_root_reads_its_storage_in_octets_and_its_message_count_rfc9208_4_1_2() {
    let lines = quota_lines("User quota", (420, 1024), (17, 1000));
    let roots = read(quota(Some(lines))).await.unwrap();
    assert_eq!(
        roots,
        [QuotaRoot {
            name: "User quota".to_owned(),
            storage: Some(Usage {
                used: 420 * KIB,
                limit: 1024 * KIB,
            }),
            messages: Some(Usage {
                used: 17,
                limit: 1000,
            }),
        }]
    );
}

#[tokio::test]
async fn gmail_names_one_root_without_a_name_over_its_storage() {
    let roots = read(Mailboxes::gmail(Vec::new())).await.unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].name, "");
    assert!(roots[0].storage.is_some() && roots[0].messages.is_none());
}

#[tokio::test]
async fn an_inbox_under_no_root_has_no_quota_and_a_server_without_quota_refuses() {
    assert_eq!(read(quota(None)).await.unwrap(), []);
    let refused = read(Mailboxes::dovecot()).await;
    assert!(matches!(
        refused,
        Err(SessionError::Protocol("the server answered BAD"))
    ));
}

#[tokio::test]
async fn a_quota_line_a_server_could_send_to_hurt_is_a_protocol_failure_rfc9208_7() {
    let root = "* QUOTAROOT INBOX \"\"\r\n";
    let unparsed = "the answer could not be parsed";
    let past = "a quota value passes its bound";
    let long_name = "r".repeat(MAX_WIRE_NAME_BYTES + 1);
    for (lines, words) in [
        // A value past 64 bits, then past 63 and in octets past 64.
        (
            format!("{root}* QUOTA \"\" (STORAGE 1 18446744073709551616)\r\n"),
            unparsed,
        ),
        (
            format!("{root}* QUOTA \"\" (STORAGE 1 9223372036854775808)\r\n"),
            past,
        ),
        (
            format!("{root}* QUOTA \"\" (STORAGE 18014398509481984 1)\r\n"),
            past,
        ),
        // A negative limit, then none.
        (format!("{root}* QUOTA \"\" (STORAGE 10 -1)\r\n"), unparsed),
        (format!("{root}* QUOTA \"\" (STORAGE 10)\r\n"), unparsed),
        // A root name past the bytes a name may take.
        (
            format!(
                "* QUOTAROOT INBOX {{{}}}\r\n{long_name}\r\n",
                long_name.len()
            ),
            "a quota root name cannot be kept",
        ),
        (
            "* QUOTA \"\" (STORAGE 1 2)\r\n".to_owned(),
            "GETQUOTAROOT answered without QUOTAROOT",
        ),
    ] {
        let read = read(quota(Some(lines.clone()))).await;
        assert!(
            matches!(read, Err(SessionError::Protocol(found)) if found == words),
            "{lines:?}: {read:?}"
        );
    }
}
