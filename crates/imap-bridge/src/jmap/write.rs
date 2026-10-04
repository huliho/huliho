// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The keywords an `Email/set` asked for, stored on the server between
//! two passes over the request: per store folder a SELECT, then one
//! STORE per set of messages that gain or lose the same flags, on the
//! account's conversation under the lock and the deadline. What the
//! server took is then written to the rows as one state. A write that
//! fails leaves its rows as they were; the next refresh reads what the
//! server holds.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::{Instant, timeout};

use super::set::{Edit, MAX_UPDATE_KEYWORD_BYTES, Outcome, SetAsk, SetError};
use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link, Wire};
use crate::session::{MAX_STORE_UIDS, Session, SessionError, StoreAsk, Writable};
use crate::store::{EmailId, KeywordChange, KeywordRow, MailboxId, MailboxRow};
use crate::sync::{Cache, blocking, mapping};

/// One email whose message a STORE names.
struct Member {
    /// Where its result stands among the updates of the ask.
    index: usize,
    id: EmailId,
    uid: u32,
}

/// The messages of one folder that gain and lose the same keywords.
struct Group {
    edit: Edit,
    /// The keywords the messages lose: the ones the edit clears and,
    /// for a whole edit, the ones a row held beyond it.
    remove: Vec<String>,
    members: Vec<Member>,
}

/// What one ask works through: the groups by folder, the folder rows
/// the write selects by, the result of every update and what the server
/// took.
struct Work {
    folders: BTreeMap<MailboxId, Vec<Group>>,
    rows: HashMap<MailboxId, MailboxRow>,
    results: Vec<(String, Result<(), SetError>)>,
    stored: Vec<KeywordChange>,
    state: u64,
}

/// Stores what the ask names and answers what came of every update. A
/// state other than the one `ifInState` names ends it before anything
/// is sent. The conversation is held from that check until the rows are
/// written, so two writes of one account never pass on one state. The
/// writes of one round share a deadline: `until` is set by the first
/// that goes to the server and a write that finds it over sends nothing.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(super) async fn apply<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    ask: &SetAsk,
    until: &mut Option<Instant>,
) -> Result<Outcome, SyncError> {
    let mut wire = link.wire.lock().await;
    let Some(mut work) = planned(cache, ask).await? else {
        return Ok(Outcome::StateMismatch);
    };
    if !work.folders.is_empty() {
        let until = *until.get_or_insert_with(|| Instant::now() + wire.deadline());
        let left = until.saturating_duration_since(Instant::now());
        send(&mut wire, cache, &mut work, left).await;
    }
    let Work {
        mut results,
        stored,
        state,
        ..
    } = work;
    if stored.is_empty() {
        return Ok(Outcome::Stored {
            old_state: state,
            new_state: state,
            results,
        });
    }
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let written = blocking(move || store.write_keywords(&key, &stored)).await?;
    for (id, result) in &mut results {
        if written.gone.iter().any(|gone| gone.as_str() == id) {
            *result = Err(SetError::NotFound);
        }
    }
    Ok(Outcome::Stored {
        old_state: written.before,
        new_state: written.after,
        results,
    })
}

/// Sends the work on the account's session within what is left of the
/// deadline; with none of it left the session is not asked at all. A
/// deadline that fires reads as a session that does not stand.
async fn send<C: Connector>(wire: &mut Wire<C>, cache: &Cache, work: &mut Work, left: Duration) {
    if left.is_zero() {
        return;
    }
    let run = async {
        let Ok(session) = wire.session(cache).await else {
            return false;
        };
        store(session, work).await
    };
    if !timeout(left, run).await.unwrap_or(false) {
        wire.drop_session();
    }
}

/// Where the emails lie and what each update asks of its message;
/// `None` when the state is not the one the ask names. An id the
/// account does not hold is not found, a row no UID names cannot be
/// written now and an update that names no keyword is done.
async fn planned(cache: &Cache, ask: &SetAsk) -> Result<Option<Work>, SyncError> {
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let ids: Vec<String> = ask.updates.iter().map(|(id, _)| id.clone()).collect();
    let (state, found, rows) = blocking(move || {
        let names: Vec<&str> = ids.iter().map(String::as_str).collect();
        let (state, found) = store.keyword_rows(&key, &names)?;
        let rows: HashMap<MailboxId, MailboxRow> = store
            .mailbox_snapshot(&key)?
            .rows
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        Ok((state, found, rows))
    })
    .await?;
    if ask
        .if_in_state
        .as_ref()
        .is_some_and(|named| *named != state.to_string())
    {
        return Ok(None);
    }
    let mut work = Work {
        folders: BTreeMap::new(),
        rows,
        results: Vec::with_capacity(ask.updates.len()),
        stored: Vec::new(),
        state,
    };
    for (index, (id, edit)) in ask.updates.iter().enumerate() {
        let row = found.iter().find(|row| row.id.as_str() == id);
        let result = match row.map(|row| (row, row.uid)) {
            None => Err(SetError::NotFound),
            Some((_, None)) => Err(SetError::ServerUnavailable),
            Some((row, Some(uid))) => {
                let remove = removed(edit, row);
                if edit.set.is_empty() && remove.is_empty() {
                    Ok(())
                } else {
                    let member = Member {
                        index,
                        id: row.id.clone(),
                        uid,
                    };
                    let groups = work.folders.entry(row.folder.clone()).or_default();
                    join(groups, edit, remove, member);
                    // Stands until the server took the flags.
                    Err(SetError::ServerUnavailable)
                }
            }
        };
        work.results.push((id.clone(), result));
    }
    Ok(Some(work))
}

/// The keywords the message of this row loses under the edit.
fn removed(edit: &Edit, row: &KeywordRow) -> Vec<String> {
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
fn join(groups: &mut Vec<Group>, edit: &Edit, remove: Vec<String>, member: Member) {
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

/// Every folder on one session. Whether the session still stands; a
/// failure that ends it leaves what comes after it as it was.
async fn store<S: Session>(session: &mut S, work: &mut Work) -> bool {
    let folders = std::mem::take(&mut work.folders);
    for (folder, groups) in folders {
        if !folder_stored(session, work, &folder, groups).await {
            return false;
        }
    }
    true
}

/// One folder: SELECT, then its groups in order. Whether the session
/// still stands.
async fn folder_stored<S: Session>(
    session: &mut S,
    work: &mut Work,
    folder: &MailboxId,
    mut groups: Vec<Group>,
) -> bool {
    // A folder gone from the store or renumbered cannot be written now.
    let Some(facts) = work.rows.get(folder).map(|row| &row.facts) else {
        return true;
    };
    let (name, uid_validity) = (facts.imap_name.clone(), facts.uid_validity);
    let writable = match session.select(&name).await {
        Ok(writable) if Some(writable.uid_validity) == uid_validity => writable,
        Ok(_) => return true,
        Err(error) => {
            let (ends, stands) = failure(&error);
            for group in &groups {
                settle(work, &group.members, &ends);
            }
            return stands;
        }
    };
    for group in &mut groups {
        if let Err(error) = lasting(&writable, group) {
            settle(work, &group.members, &error);
            continue;
        }
        group.members.sort_unstable_by_key(|member| member.uid);
        if !group_stored(session, work, folder, group).await {
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
    for members in group.members.chunks(MAX_STORE_UIDS) {
        match commands(session, group, members).await {
            Ok(()) => keep(work, folder, group, members),
            Err(error) => {
                let (ends, stands) = failure(&error);
                settle(work, members, &ends);
                if !stands {
                    return false;
                }
            }
        }
    }
    true
}

/// What a failed SELECT or STORE comes to: the error its updates end
/// with and whether the session still stands. A NO is the server's
/// refusal and keeps the session, an answer that cannot be read is its
/// failure and costs it, anything else leaves the server unreached.
fn failure(error: &SessionError) -> (SetError, bool) {
    match error {
        SessionError::Refused => (SetError::ServerFail, true),
        SessionError::Protocol(_) => (SetError::ServerFail, false),
        _ => (SetError::ServerUnavailable, false),
    }
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
        for flags in runs(&flags) {
            let ask = StoreAsk {
                uids: &uids,
                flags,
                add,
            };
            session.uid_store(&ask).await?;
        }
    }
    Ok(())
}

/// The flags in runs of `MAX_UPDATE_KEYWORD_BYTES` at most, each flag
/// with its separator, so a STORE of one run over a full set of UIDs
/// stays on one command line. A whole edit removes what the row held,
/// which the bound on what an update names does not cover.
fn runs(flags: &[String]) -> Vec<&[String]> {
    let mut runs = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (index, flag) in flags.iter().enumerate() {
        let wide = flag.len() + 1;
        if bytes + wide > MAX_UPDATE_KEYWORD_BYTES && index > start {
            runs.push(&flags[start..index]);
            (start, bytes) = (index, 0);
        }
        bytes += wide;
    }
    if start < flags.len() {
        runs.push(&flags[start..]);
    }
    runs
}

/// The server took the set: its updates are done once the rows are
/// written.
fn keep(work: &mut Work, folder: &MailboxId, group: &Group, members: &[Member]) {
    for member in members {
        work.results[member.index].1 = Ok(());
        work.stored.push(KeywordChange {
            id: member.id.clone(),
            folder: folder.clone(),
            uid: member.uid,
            add: group.edit.set.clone(),
            remove: group.remove.clone(),
        });
    }
}

/// Every update of these members ends with the error.
fn settle(work: &mut Work, members: &[Member], error: &SetError) {
    for member in members {
        work.results[member.index].1 = Err(error.clone());
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
        let found = runs(&flags);
        assert!(found.len() > 1);
        for run in &found {
            let bytes: usize = run.iter().map(|flag| flag.len() + 1).sum();
            assert!(bytes <= MAX_UPDATE_KEYWORD_BYTES, "{bytes}");
        }
        assert_eq!(found.concat(), flags);
        assert_eq!(runs(&flags[..3]), [&flags[..3]]);
        assert!(runs(&[]).is_empty());
    }
}
