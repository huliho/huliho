// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The keywords of an `Email/set` on one folder: one STORE per set of
//! messages that gain or lose the same flags (RFC 3501 section 6.4.6).

use super::super::set::{Edit, MAX_UPDATE_KEYWORD_BYTES, SetError};
use super::{Member, Work, failure};
use crate::session::{MAX_STORE_UIDS, Session, SessionError, StoreAsk, StoreItem, Writable};
use crate::store::{KeywordChange, KeywordRow, MailboxId};
use crate::sync::mapping;

/// The messages of one folder that gain and lose the same keywords.
pub(super) struct Group {
    pub edit: Edit,
    /// The keywords the messages lose: the ones the edit clears and,
    /// for a whole edit, the ones a row held beyond it.
    pub remove: Vec<String>,
    pub members: Vec<Member>,
}

/// The keywords the message of this row loses under the edit.
pub(super) fn removed(edit: &Edit, row: &KeywordRow) -> Vec<String> {
    if !edit.whole {
        return edit.clear.clone();
    }
    row.keywords
        .keys()
        .filter(|keyword| !edit.set.contains(keyword))
        .cloned()
        .collect()
}

/// Puts the member with the others that gain and lose the same.
pub(super) fn join(groups: &mut Vec<Group>, edit: &Edit, remove: Vec<String>, member: Member) {
    let same = groups
        .iter_mut()
        .find(|group| group.edit == *edit && group.remove == remove);
    match same {
        Some(group) => group.members.push(member),
        None => groups.push(Group {
            edit: edit.clone(),
            remove,
            members: vec![member],
        }),
    }
}

/// The groups of one selected folder in order. Whether the session
/// still stands.
pub(super) async fn stored<S: Session>(
    session: &mut S,
    work: &mut Work,
    (folder, writable): (&MailboxId, &Writable),
    groups: Vec<Group>,
) -> bool {
    for group in groups {
        if let Err(error) = lasting(writable, &group) {
            work.settle(&group.members, &error);
            continue;
        }
        if !group_stored(session, work, folder, &group).await {
            return false;
        }
    }
    true
}

/// One group over sets of `MAX_STORE_UIDS` messages in the order of
/// their UIDs, so neighbors fold into ranges. A set the server took is
/// kept whatever comes of the next one. Whether the session still
/// stands.
async fn group_stored<S: Session>(
    session: &mut S,
    work: &mut Work,
    folder: &MailboxId,
    group: &Group,
) -> bool {
    for members in work.standing(&group.members).chunks(MAX_STORE_UIDS) {
        match commands(session, group, members).await {
            Ok(()) => keep(work, folder, group, members),
            Err(error) => {
                let (ends, stands) = failure(&error);
                work.settle(members, &ends);
                if !stands {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether the folder keeps every flag of the group for good: a system
/// flag it does not keep is forbidden, a keyword it does not keep is an
/// invalid property under the path the patch named it by.
fn lasting(writable: &Writable, group: &Group) -> Result<(), SetError> {
    let named = || group.edit.set.iter().chain(&group.remove);
    let lost = |keyword: &&String| !writable.keeps(&mapping::flag(keyword));
    if named()
        .filter(lost)
        .any(|keyword| mapping::is_system_flag(&mapping::flag(keyword)))
    {
        return Err(SetError::Forbidden);
    }
    let paths: Vec<String> = named()
        .filter(lost)
        .map(|keyword| group.edit.path(keyword))
        .collect();
    if paths.is_empty() {
        Ok(())
    } else {
        Err(SetError::InvalidProperties(paths))
    }
}

/// The STOREs of one set of messages: what they gain, then what they
/// lose, the flags of each in runs that fit a command line.
async fn commands<S: Session>(
    session: &mut S,
    group: &Group,
    members: &[Member],
) -> Result<(), SessionError> {
    let uids: Vec<u32> = members.iter().map(|member| member.uid).collect();
    for (keywords, add) in [(&group.edit.set, true), (&group.remove, false)] {
        let flags: Vec<String> = keywords
            .iter()
            .map(|keyword| mapping::flag(keyword))
            .collect();
        for flags in runs(&flags, str::len) {
            let ask = StoreAsk {
                uids: &uids,
                flags,
                add,
                item: StoreItem::Flags,
            };
            session.uid_store(&ask).await?;
        }
    }
    Ok(())
}

/// The items in runs of `MAX_UPDATE_KEYWORD_BYTES` at most, each as wide
/// as `width` says plus its separator, so a STORE of one run over a full
/// set of UIDs stays on one command line. The bound on what an update
/// names covers neither what a whole edit removes nor the labels of a
/// relabel.
pub(super) fn runs(items: &[String], width: impl Fn(&str) -> usize) -> Vec<&[String]> {
    let mut runs = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (index, item) in items.iter().enumerate() {
        let wide = width(item) + 1;
        if bytes + wide > MAX_UPDATE_KEYWORD_BYTES && index > start {
            runs.push(&items[start..index]);
            (start, bytes) = (index, 0);
        }
        bytes += wide;
    }
    if start < items.len() {
        runs.push(&items[start..]);
    }
    runs
}

/// Whether an item of that width fits a run on its own, separator
/// included.
pub(super) fn fits(width: usize) -> bool {
    width < MAX_UPDATE_KEYWORD_BYTES
}

/// The server took the set: its keyword parts are done once the rows
/// are written.
fn keep(work: &mut Work, folder: &MailboxId, group: &Group, members: &[Member]) {
    for member in members {
        work.done(member);
        work.stored.keywords.push(KeywordChange {
            id: member.id.clone(),
            folder: folder.clone(),
            uid: member.uid,
            add: group.edit.set.clone(),
            remove: group.remove.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The longest keyword (RFC 8621 section 4.1.1).
    const LONGEST: usize = 255;

    #[test]
    fn flags_past_one_command_line_go_out_in_runs_that_fit() {
        let flags: Vec<String> = (0..40).map(|n| format!("{n:0LONGEST$}")).collect();
        let found = runs(&flags, str::len);
        assert!(found.len() > 1);
        for run in &found {
            let bytes: usize = run.iter().map(|flag| flag.len() + 1).sum();
            assert!(bytes <= MAX_UPDATE_KEYWORD_BYTES, "{bytes}");
        }
        assert_eq!(found.concat(), flags);
        assert_eq!(runs(&flags[..3], str::len), [&flags[..3]]);
        assert!(runs(&[], str::len).is_empty(), "{:?}", runs(&[], str::len));
        assert!(fits(LONGEST));
        assert!(fits(MAX_UPDATE_KEYWORD_BYTES - 1));
        assert!(!fits(MAX_UPDATE_KEYWORD_BYTES));
    }
}
