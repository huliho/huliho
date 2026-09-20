// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/get` (RFC 8621 section 2.1) over the rows the pass wrote.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Context, MAX_OBJECTS_IN_GET, MethodError, arguments};
use crate::store::{MailboxRow, MailboxSnapshot};

/// The property the vendor capability adds: the emails the bridge holds
/// for the mailbox, next to `totalEmails` from the server.
const SYNCED_EMAILS: &str = "syncedEmails";

/// The properties of RFC 8621 section 2.
const PROPERTIES: [&str; 11] = [
    "id",
    "name",
    "parentId",
    "role",
    "sortOrder",
    "totalEmails",
    "unreadEmails",
    "totalThreads",
    "unreadThreads",
    "myRights",
    "isSubscribed",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetArguments {
    account_id: String,
    ids: Option<Vec<String>>,
    properties: Option<Vec<String>>,
}

/// `Mailbox/get`: the named rows or every row when `ids` is null, each
/// cut to the wanted properties, with the ids that name nothing.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    if arguments
        .ids
        .as_ref()
        .is_some_and(|ids| ids.len() > MAX_OBJECTS_IN_GET)
    {
        return Err(MethodError::RequestTooLarge);
    }
    let wanted = properties(arguments.properties.as_deref(), context.using.huliho)?;
    let snapshot = context.store.mailbox_snapshot(context.key)?;
    let mut list = Vec::new();
    let mut not_found: Vec<&str> = Vec::new();
    match &arguments.ids {
        None => list.extend(
            snapshot
                .rows
                .iter()
                .map(|row| object(row, &snapshot, &wanted)),
        ),
        Some(ids) => {
            // RFC 8620 section 5.1: an id sent more than once is answered once.
            let mut seen = HashSet::new();
            for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
                match snapshot.rows.iter().find(|row| row.id.as_str() == id) {
                    Some(row) => list.push(object(row, &snapshot, &wanted)),
                    None => not_found.push(id),
                }
            }
        }
    }
    let mut answer = Map::new();
    answer.insert("accountId".to_owned(), Value::from(context.key.as_str()));
    answer.insert("state".to_owned(), Value::from(snapshot.state.to_string()));
    answer.insert("list".to_owned(), Value::Array(list));
    answer.insert("notFound".to_owned(), json!(not_found));
    Ok(answer)
}

/// The properties to render: every one when none is named, the named
/// ones plus `id` otherwise; `syncedEmails` exists only under the
/// vendor capability.
fn properties(named: Option<&[String]>, huliho: bool) -> Result<Vec<&'static str>, MethodError> {
    let Some(named) = named else {
        let mut all = PROPERTIES.to_vec();
        if huliho {
            all.push(SYNCED_EMAILS);
        }
        return Ok(all);
    };
    let mut wanted = vec!["id"];
    for name in named {
        let property = PROPERTIES
            .iter()
            .copied()
            .find(|known| *known == name)
            .or_else(|| (huliho && name == SYNCED_EMAILS).then_some(SYNCED_EMAILS))
            .ok_or(MethodError::InvalidArguments(
                "an unknown property was asked for",
            ))?;
        if !wanted.contains(&property) {
            wanted.push(property);
        }
    }
    Ok(wanted)
}

/// One Mailbox object (RFC 8621 section 2), cut to the wanted
/// properties. The email counts are what STATUS said until the first
/// sync of the folder is done and what the memberships say afterwards:
/// STATUS counts a message flagged `\Deleted` and an unseen draft,
/// which JMAP does not (RFC 8621 sections 2 and 4.1.1).
fn object(row: &MailboxRow, snapshot: &MailboxSnapshot, wanted: &[&str]) -> Value {
    let facts = &row.facts;
    let counted = snapshot.counts.get(&row.id).copied().unwrap_or_default();
    let (total_emails, unread_emails) = if snapshot.done.contains(&row.id) {
        (counted.synced_emails, counted.unread_emails)
    } else {
        (facts.total_emails, facts.unread_emails)
    };
    let pairs = [
        ("id", json!(row.id)),
        ("name", json!(facts.name)),
        ("parentId", json!(row.parent_id)),
        ("role", json!(facts.role)),
        ("sortOrder", json!(facts.sort_order)),
        ("totalEmails", json!(total_emails)),
        ("unreadEmails", json!(unread_emails)),
        ("totalThreads", json!(counted.total_threads)),
        ("unreadThreads", json!(counted.unread_threads)),
        ("myRights", rights(facts.selectable)),
        ("isSubscribed", json!(facts.subscribed)),
        (SYNCED_EMAILS, json!(counted.synced_emails)),
    ];
    pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into()
}

/// The rights of a read-only account: reading where the mailbox can be
/// selected, nothing else until a write method exists.
fn rights(selectable: bool) -> Value {
    json!({
        "mayReadItems": selectable,
        "mayAddItems": false,
        "mayRemoveItems": false,
        "maySetSeen": false,
        "maySetKeywords": false,
        "mayCreateChild": false,
        "mayRename": false,
        "mayDelete": false,
        "maySubmit": false,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::store::{MailboxFacts, MailboxId};

    #[test]
    fn a_row_nothing_can_select_and_nobody_subscribed_reads_as_such_rfc8621_2() {
        let row = MailboxRow {
            id: MailboxId::generate(),
            parent_id: None,
            facts: MailboxFacts {
                name: "Lists".to_owned(),
                imap_name: "Lists".to_owned(),
                parent_imap_name: None,
                role: None,
                sort_order: 10,
                subscribed: false,
                selectable: false,
                store: false,
                gmail_label: None,
                uid_validity: None,
                uid_next: None,
                highest_modseq: None,
                total_emails: 0,
                unread_emails: 0,
            },
        };
        let snapshot = MailboxSnapshot {
            state: 0,
            rows: Vec::new(),
            counts: HashMap::new(),
            done: HashSet::new(),
        };
        let rendered = object(&row, &snapshot, &["myRights", "isSubscribed"]);
        assert_eq!(rendered["myRights"]["mayReadItems"], false);
        assert_eq!(rendered["isSubscribed"], false);
        assert_eq!(rendered.as_object().unwrap().len(), 2);
    }

    #[test]
    fn properties_default_to_all_and_id_always_rides_along_rfc8620_5_1() {
        assert_eq!(properties(None, false).unwrap().len(), PROPERTIES.len());
        assert_eq!(properties(None, true).unwrap().last(), Some(&SYNCED_EMAILS));
        let named = ["name".to_owned(), "role".to_owned(), "name".to_owned()];
        assert_eq!(
            properties(Some(&named), false).unwrap(),
            ["id", "name", "role"]
        );
        let vendor = ["syncedEmails".to_owned()];
        assert!(matches!(
            properties(Some(&vendor), false),
            Err(MethodError::InvalidArguments(_))
        ));
        assert_eq!(
            properties(Some(&vendor), true).unwrap(),
            ["id", "syncedEmails"]
        );
    }
}
