// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox pass: what LIST, STATUS and MYRIGHTS say about an
//! account, mapped to rows with the roles the user set and written under
//! one new state.

mod mapping;
pub mod rights;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;

use crate::gmail;
use crate::session::{
    Acl, Capabilities, ListEntry, ListReturn, Listing, Moving, Session, SessionError, StatusEntry,
    StatusItems,
};
use crate::store::{MailboxFacts, Pass, PassInputs, StoreError};
use crate::sync::{Cache, blocking};

pub(crate) use mapping::sort_order;
pub use mapping::{ROLES, Subscriptions, map, map_with_overrides};

/// The one name RFC 3501 section 5.1 fixes, in any case.
pub(crate) const INBOX: &str = "INBOX";

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

/// Reads the mailbox list with its counts and the rights of every
/// mailbox, writes the difference to the store, then the quota of the
/// inbox; answers the account's state afterwards.
///
/// # Errors
///
/// Returns the session's failure for a refused command, a lost
/// connection or a listing without a mailbox, the store's for a write
/// that fails and `Task` when the blocking task that writes the store
/// ends early. A STATUS or a MYRIGHTS that one mailbox answers with NO
/// is no failure: that mailbox goes without counts or rights. A quota
/// the server cannot describe fails after the rows are written.
pub async fn sync<S: Session>(session: &mut S, cache: &Cache) -> Result<u64, SyncError> {
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let inputs = blocking(move || store.pass_inputs(&key)).await?;
    let found = observe(session, cache.gmail, &inputs, true).await?;
    let advertised = found.quota;
    let state = write(cache, found).await?;
    quota(session, cache, advertised).await?;
    Ok(state)
}

/// What one pass reads of the server.
pub(crate) struct Observed {
    pub facts: Vec<MailboxFacts>,
    /// The commands the server offers for moving mail, which the rights
    /// of every mailbox follow.
    pub moving: Moving,
    /// Whether the server advertises QUOTA (RFC 9208).
    pub quota: bool,
    /// The letters MYRIGHTS named, by wire name, where the server
    /// advertises ACL.
    pub rights: Option<HashMap<String, String>>,
}

/// Writes what a pass read as one state.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(crate) async fn write(cache: &Cache, found: Observed) -> Result<u64, SyncError> {
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    blocking(move || {
        let pass = Pass {
            found: &found.facts,
            moving: found.moving,
            rights: found.rights.as_ref(),
        };
        store.apply_pass(&key, pass)
    })
    .await
}

/// GETQUOTAROOT of the inbox where QUOTA is advertised, kept by the
/// store; a refusal reads as an inbox under no root. Where QUOTA is not
/// advertised the account has no quota.
pub(crate) async fn quota<S: Session>(
    session: &mut S,
    cache: &Cache,
    advertised: bool,
) -> Result<(), SessionError> {
    let roots = if advertised {
        match session.quota_root().await {
            Ok(roots) => Some(roots),
            Err(SessionError::Refused) => Some(Vec::new()),
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    cache.store.record_quota(&cache.key, roots.as_deref());
    Ok(())
}

/// The listing in the form the capabilities allow, then STATUS per
/// selectable mailbox where the listing did not carry it and, where ACL
/// is advertised, the rights, mapped to facts with the roles the user
/// set. `gmail` is the host's word on the account, which holds once the
/// server confirms it. With `every_right` MYRIGHTS goes out for every
/// selectable mailbox where the listing did not carry the rights, as on
/// the first pass of a connection; otherwise only for one whose row
/// holds none, the others keeping theirs.
pub(crate) async fn observe<S: Session>(
    session: &mut S,
    gmail: bool,
    inputs: &PassInputs,
    every_right: bool,
) -> Result<Observed, SessionError> {
    let capabilities = session.capabilities().await?;
    let gmail = gmail::confirmed(gmail, &capabilities);
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
    let acl = capabilities.acl();
    let rights = if acl.my_rights {
        let asked = if options.my_rights {
            None
        } else {
            Some(Asked {
                held: &inputs.rights,
                every_right,
            })
        };
        Some(read_rights(session, &listing, acl, asked).await?)
    } else {
        None
    };
    let facts = mapping::map_with_overrides(&listing, &subscriptions, gmail, inputs);
    Ok(Observed {
        facts,
        moving: capabilities.moving(),
        quota: capabilities.has("QUOTA"),
        rights,
    })
}

/// Which mailboxes a pass asks MYRIGHTS of, one by one: every selectable
/// one, or only those whose row holds no letters.
struct Asked<'a> {
    /// The letters each row holds, by wire name.
    held: &'a HashMap<String, String>,
    every_right: bool,
}

/// The letters of every mailbox the listing carried them for, else of
/// every selectable mailbox, through MYRIGHTS one by one where the ask
/// names it and from its row otherwise. A mailbox that answers NO holds
/// every right, since the server cannot say which (RFC 4314 section
/// 3.5). Kept as the row keeps them.
async fn read_rights<S: Session>(
    session: &mut S,
    listing: &Listing,
    acl: Acl,
    asked: Option<Asked<'_>>,
) -> Result<HashMap<String, String>, SessionError> {
    let obsolete = !acl.current;
    let mut found: HashMap<String, String> = listing
        .rights
        .iter()
        .map(|entry| (entry.mailbox.clone(), rights::kept(&entry.rights, obsolete)))
        .collect();
    let Some(asked) = asked else {
        return Ok(found);
    };
    for entry in &listing.entries {
        if !mapping::is_selectable(&entry.attributes) {
            continue;
        }
        let held = asked.held.get(&entry.name).filter(|_| !asked.every_right);
        let letters = match held {
            Some(letters) => letters.clone(),
            None => match session.my_rights(&entry.name).await {
                Ok(rights) => rights::kept(&rights, obsolete),
                Err(SessionError::Refused) => rights::EVERY_RIGHT.to_owned(),
                Err(other) => return Err(other),
            },
        };
        found.insert(entry.name.clone(), letters);
    }
    Ok(found)
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
/// section 2, RFC 5819, RFC 8440).
fn list_return(capabilities: &Capabilities) -> ListReturn {
    let extended = capabilities.has("LIST-EXTENDED");
    let acl = capabilities.acl();
    ListReturn {
        subscribed: extended,
        special_use: extended && capabilities.has("SPECIAL-USE"),
        status: (extended && capabilities.has("LIST-STATUS")).then(|| status_items(capabilities)),
        my_rights: extended && acl.my_rights && acl.list_rights,
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
                my_rights: false,
            }
        );
        let plain = advertised(&["IMAP4rev1"]);
        assert_eq!(list_return(&plain), ListReturn::default());
        assert_eq!(status_items(&plain), StatusItems { modseq: false });
        let special_only = advertised(&["IMAP4rev1", "SPECIAL-USE", "LIST-STATUS"]);
        assert_eq!(list_return(&special_only), ListReturn::default());
    }

    #[test]
    fn the_rights_ride_the_listing_where_list_myrights_stands_beside_acl_rfc8440() {
        let cyrus = advertised(&["IMAP4rev1", "LIST-EXTENDED", "ACL", "LIST-MYRIGHTS"]);
        assert!(list_return(&cyrus).my_rights);
        let one_by_one = advertised(&["IMAP4rev1", "LIST-EXTENDED", "ACL"]);
        assert!(!list_return(&one_by_one).my_rights);
        let without_acl = advertised(&["IMAP4rev1", "LIST-EXTENDED", "LIST-MYRIGHTS"]);
        assert!(!list_return(&without_acl).my_rights);
        let plain = advertised(&["IMAP4rev1", "ACL", "LIST-MYRIGHTS"]);
        assert!(!list_return(&plain).my_rights);
    }

    #[test]
    fn a_name_the_listing_repeats_is_kept_once_where_it_first_stood() {
        let kept = distinct(vec![listed("INBOX"), listed("Work"), listed("INBOX")]);
        let names: Vec<&str> = kept.iter().map(|found| found.name.as_str()).collect();
        assert_eq!(names, ["INBOX", "Work"]);
    }
}
