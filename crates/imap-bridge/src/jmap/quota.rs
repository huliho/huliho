// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Quota/get` and `Quota/changes` (RFC 9425 sections 4.2 and 4.3) over
//! the quota the last mailbox pass read: one Quota per resource of a
//! root, STORAGE as `octets` and MESSAGE as `count`; each counts the
//! type `Email` in the scope `account`. `Quota/query` and
//! `Quota/queryChanges` are not served.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::changes::{self, ChangesArguments};
use super::get::{self, GetArguments};
use super::{Context, MAIL_CAPABILITY, MethodError, arguments};
use crate::store::{QuotaRow, QuotaSnapshot, Resource};

/// The properties of RFC 9425 section 4.1. IMAP names no warn or soft
/// limit and no description, so those read null.
const PROPERTIES: [&str; 10] = [
    "id",
    "resourceType",
    "used",
    "hardLimit",
    "scope",
    "name",
    "types",
    "warnLimit",
    "softLimit",
    "description",
];

/// The largest `UnsignedInt` (RFC 8620 section 1.3); a usage or a limit
/// past it reads as it.
const MAX_UNSIGNED_INT: u64 = (1 << 53) - 1;

/// The type every quota of the bridge counts, from the JMAP Types Names
/// registry; the mail capability carries it.
const EMAIL_TYPE: &str = "Email";

/// The scope of a quota root over the user's inbox (RFC 9425 section
/// 3.1).
const ACCOUNT_SCOPE: &str = "account";

/// `Quota/get`: the named quotas or every one when `ids` is null, each
/// cut to the wanted properties, with the ids that name nothing.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let snapshot = served(context)?;
    // Null `ids` asks for every quota (RFC 8620 section 5.1); the roots
    // of one inbox fit one answer.
    let asked = match arguments.ids.as_deref() {
        None => Vec::new(),
        named => get::ids(named)?,
    };
    let wanted = get::properties(&PROPERTIES, arguments.properties.as_deref())?;
    // RFC 9425 section 4.1: a quota whose types the request's
    // capabilities leave empty is never returned.
    let shown: &[QuotaRow] = if context.using.has(MAIL_CAPABILITY) {
        &snapshot.rows
    } else {
        &[]
    };
    let answered: Vec<&QuotaRow> = if arguments.ids.is_none() {
        shown.iter().collect()
    } else {
        asked
            .iter()
            .filter_map(|id| shown.iter().find(|row| row.id == *id))
            .collect()
    };
    let found: HashSet<&str> = answered.iter().map(|row| row.id.as_str()).collect();
    let list = answered.iter().map(|row| object(row, &wanted)).collect();
    Ok(get::answer(
        context,
        (snapshot.state.as_str(), list),
        &asked,
        &found,
    ))
}

/// `Quota/changes`: the bridge knows the state it stands at and no
/// other, so any other state, one from before a restart among them,
/// answers `cannotCalculateChanges` and the client reads again.
pub(super) fn changes(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: ChangesArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    changes::checked(&arguments)?;
    let snapshot = served(context)?;
    if arguments.since_state != snapshot.state {
        return Err(MethodError::CannotCalculateChanges);
    }
    let none = || Value::Array(Vec::new());
    let answer = [
        ("accountId", Value::from(context.key.as_str())),
        ("oldState", Value::from(arguments.since_state)),
        ("newState", Value::from(snapshot.state)),
        ("hasMoreChanges", Value::from(false)),
        ("created", none()),
        ("updated", none()),
        ("destroyed", none()),
        ("updatedProperties", Value::Null),
    ];
    Ok(answer
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

/// The account's quota; an account whose server does not advertise
/// QUOTA has none to serve.
fn served(context: &Context<'_>) -> Result<QuotaSnapshot, MethodError> {
    let snapshot = context.store.quota(context.key);
    if snapshot.advertised {
        Ok(snapshot)
    } else {
        Err(MethodError::AccountNotSupportedByMethod)
    }
}

/// One Quota object (RFC 9425 section 4.1), cut to the wanted
/// properties.
fn object(row: &QuotaRow, wanted: &[&str]) -> Value {
    let resource_type = match row.resource {
        Resource::Storage => "octets",
        Resource::Messages => "count",
    };
    let pairs = [
        ("id", json!(row.id)),
        ("resourceType", json!(resource_type)),
        ("used", json!(row.usage.used.min(MAX_UNSIGNED_INT))),
        ("hardLimit", json!(row.usage.limit.min(MAX_UNSIGNED_INT))),
        ("scope", json!(ACCOUNT_SCOPE)),
        ("name", json!(row.root)),
        ("types", json!([EMAIL_TYPE])),
        ("warnLimit", Value::Null),
        ("softLimit", Value::Null),
        ("description", Value::Null),
    ];
    pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Usage;

    fn row(resource: Resource, used: u64, limit: u64) -> QuotaRow {
        QuotaRow {
            id: "q1".to_owned(),
            root: "User quota".to_owned(),
            resource,
            usage: Usage { used, limit },
        }
    }

    #[test]
    fn storage_reads_as_octets_and_messages_as_a_count_rfc9425_3_2() {
        let all = get::properties(&PROPERTIES, None).unwrap();
        let octets = object(&row(Resource::Storage, 420, 1024), &all);
        assert_eq!(
            octets,
            json!({
                "id": "q1",
                "resourceType": "octets",
                "used": 420,
                "hardLimit": 1024,
                "scope": "account",
                "name": "User quota",
                "types": ["Email"],
                "warnLimit": null,
                "softLimit": null,
                "description": null,
            })
        );
        let count = object(&row(Resource::Messages, 17, 1000), &all);
        assert_eq!(count["resourceType"], "count");
    }

    #[test]
    fn a_value_past_the_largest_unsigned_int_reads_as_it_rfc8620_1_3() {
        let all = get::properties(&PROPERTIES, None).unwrap();
        let huge = object(&row(Resource::Storage, u64::MAX, u64::MAX - 1), &all);
        assert_eq!(huge["used"], MAX_UNSIGNED_INT);
        assert_eq!(huge["hardLimit"], MAX_UNSIGNED_INT);
        let edge = object(&row(Resource::Storage, MAX_UNSIGNED_INT, 0), &all);
        assert_eq!(edge["used"], MAX_UNSIGNED_INT);
    }

    #[test]
    fn named_properties_cut_the_object_and_id_rides_along_rfc8620_5_1() {
        let named = ["used".to_owned(), "hardLimit".to_owned()];
        let wanted = get::properties(&PROPERTIES, Some(&named)).unwrap();
        let cut = object(&row(Resource::Storage, 1, 2), &wanted);
        assert_eq!(cut, json!({ "id": "q1", "used": 1, "hardLimit": 2 }));
    }
}
