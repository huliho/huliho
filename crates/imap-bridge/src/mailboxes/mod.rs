// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox pass: what LIST and STATUS say about an account, mapped
//! to rows and written under one new state.

mod mapping;

use std::collections::HashSet;
use std::sync::Arc;

use thiserror::Error;

use crate::session::{
    Capabilities, ListEntry, ListReturn, Session, SessionError, StatusEntry, StatusItems,
};
use crate::store::{AccountKey, MailboxFacts, Store, StoreError};

pub use mapping::{ROLES, Subscriptions, map};

/// Why a pass failed: the server or the store.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the task writing the store ended early")]
    Task,
}

/// Reads the mailbox list with its counts and writes the difference to
/// the store; answers the account's state afterwards.
///
/// # Errors
///
/// Returns the session's failure for a refused command, a lost
/// connection or a listing without a mailbox, the store's for a write
/// that fails and `Task` when the blocking task that writes the store
/// ends early. A STATUS that one mailbox answers with NO is no failure:
/// that mailbox goes without counts.
pub async fn sync<S: Session>(
    session: &mut S,
    store: Arc<Store>,
    key: AccountKey,
) -> Result<u64, SyncError> {
    let found = observe(session).await?;
    tokio::task::spawn_blocking(move || store.apply_mailboxes(&key, &found))
        .await
        .map_err(|_join| SyncError::Task)?
        .map_err(SyncError::from)
}

/// The listing in the form the capabilities allow, then STATUS per
/// selectable mailbox where the listing did not carry it, mapped to
/// facts.
pub(crate) async fn observe<S: Session>(
    session: &mut S,
) -> Result<Vec<MailboxFacts>, SessionError> {
    let capabilities = session.capabilities().await?;
    let options = list_return(&capabilities);
    let mut listing = session.list(options).await?;
    listing.entries = distinct(listing.entries);
    // INBOX always exists (RFC 3501 section 5.1); an empty listing would destroy every row.
    if listing.entries.is_empty() {
        return Err(SessionError::Protocol("LIST answered without a mailbox"));
    }
    if options.status.is_none() {
        let items = status_items(&capabilities);
        listing.statuses = statuses(session, &listing.entries, items).await?;
    }
    let subscriptions = if options.subscribed {
        Subscriptions::Attributes
    } else {
        Subscriptions::Lsub(session.lsub().await?.into_iter().collect())
    };
    Ok(map(&listing.entries, &listing.statuses, &subscriptions))
}

/// STATUS for every selectable entry. A mailbox that answers NO goes
/// without one, as a LIST-STATUS answer may leave its line out
/// (RFC 5819 section 2).
async fn statuses<S: Session>(
    session: &mut S,
    entries: &[ListEntry],
    items: StatusItems,
) -> Result<Vec<StatusEntry>, SessionError> {
    let mut found = Vec::new();
    for entry in entries {
        if !mapping::is_selectable(&entry.attributes) {
            continue;
        }
        match session.status(&entry.name, items).await {
            Ok(status) => found.push(status),
            Err(SessionError::Refused) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(found)
}

/// The first entry per wire name in the order listed; the store logs a
/// mailbox once per pass.
fn distinct(mut entries: Vec<ListEntry>) -> Vec<ListEntry> {
    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.name.clone()));
    entries
}

/// The RETURN options the capabilities allow (RFC 5258, RFC 6154
/// section 2, RFC 5819).
fn list_return(capabilities: &Capabilities) -> ListReturn {
    let extended = capabilities.has("LIST-EXTENDED");
    ListReturn {
        subscribed: extended,
        special_use: extended && capabilities.has("SPECIAL-USE"),
        status: (extended && capabilities.has("LIST-STATUS")).then(|| status_items(capabilities)),
    }
}

/// HIGHESTMODSEQ only where CONDSTORE is advertised (RFC 7162 section
/// 3.1.7).
fn status_items(capabilities: &Capabilities) -> StatusItems {
    StatusItems {
        modseq: capabilities.has("CONDSTORE"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertised(names: &[&str]) -> Capabilities {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn listed(name: &str) -> ListEntry {
        ListEntry {
            name: name.to_owned(),
            delimiter: Some('/'),
            attributes: Vec::new(),
        }
    }

    #[test]
    fn the_list_form_follows_the_capabilities_rfc5258_rfc6154_2_rfc5819() {
        let dovecot = advertised(&[
            "IMAP4rev1",
            "LIST-EXTENDED",
            "LIST-STATUS",
            "SPECIAL-USE",
            "CONDSTORE",
        ]);
        assert_eq!(
            list_return(&dovecot),
            ListReturn {
                subscribed: true,
                special_use: true,
                status: Some(StatusItems { modseq: true }),
            }
        );
        let plain = advertised(&["IMAP4rev1"]);
        assert_eq!(list_return(&plain), ListReturn::default());
        assert_eq!(status_items(&plain), StatusItems { modseq: false });
        let special_only = advertised(&["IMAP4rev1", "SPECIAL-USE", "LIST-STATUS"]);
        assert_eq!(list_return(&special_only), ListReturn::default());
    }

    #[test]
    fn a_name_the_listing_repeats_is_kept_once_where_it_first_stood() {
        let kept = distinct(vec![listed("INBOX"), listed("Work"), listed("INBOX")]);
        let names: Vec<&str> = kept.iter().map(|found| found.name.as_str()).collect();
        assert_eq!(names, ["INBOX", "Work"]);
    }
}
