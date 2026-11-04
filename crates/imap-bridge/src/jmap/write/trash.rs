// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft that leaves a Gmail account's store: it goes to Trash with
//! `UID MOVE`, never with `\\Deleted` and an expunge, since the account's
//! own setting decides what an expunge does outside Spam and Trash.

use std::collections::HashMap;

use super::super::set::SetError;
use super::moves::removed;
use super::{Member, Work};
use crate::session::{MAX_STORE_UIDS, MoveAsk, Session};
use crate::store::{KeywordRow, MailboxId, MailboxRow};

/// The role of the mailbox drafts lie in.
const DRAFTS_ROLE: &str = "drafts";

/// The role of the store a draft goes to.
const TRASH_ROLE: &str = "trash";

/// Whether the row of a Gmail account is a draft: its message carries the
/// Drafts label, which is what makes it one there.
pub(super) fn is_draft(row: &KeywordRow, rows: &HashMap<MailboxId, MailboxRow>) -> bool {
    row.mailboxes.iter().any(|mailbox| {
        rows.get(mailbox)
            .is_some_and(|row| row.facts.role.as_deref() == Some(DRAFTS_ROLE))
    })
}

/// The drafts of one selected Gmail store on their way to Trash: `UID
/// MOVE` with the rows leaving as destroyed, since the next pass of Trash
/// lists the copy under a new id. Whether the session still stands.
pub(super) async fn trashed<S: Session>(
    session: &mut S,
    work: &mut Work,
    (folder, members): (&MailboxId, &[Member]),
) -> bool {
    if members.is_empty() {
        return true;
    }
    let trash = work
        .rows
        .values()
        .find(|row| row.facts.role.as_deref() == Some(TRASH_ROLE));
    let Some(target) = trash.map(|row| row.facts.imap_name.clone()) else {
        work.settle(members, &SetError::ServerUnavailable);
        return true;
    };
    for members in work.standing(members).chunks(MAX_STORE_UIDS) {
        let uids: Vec<u32> = members.iter().map(|member| member.uid).collect();
        let ask = MoveAsk {
            uids: &uids,
            target: &target,
        };
        let result = session.uid_move(&ask).await.map(drop);
        if !removed(work, folder, members, result) {
            return false;
        }
    }
    true
}
