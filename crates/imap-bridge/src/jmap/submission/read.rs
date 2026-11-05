// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What one `EmailSubmission/set` reads on its first pass: each create
//! object against the rows, refused before anything connects where it
//! can be, and the two `onSuccess` arguments (RFC 8621 section 7.5).

use serde_json::{Map, Value};

use super::super::{Context, MethodError};
use super::{Envelope, OnSuccess, SubmitError, Submitting};

/// The properties a create may name; every other one is the server's
/// or unknown (RFC 8621 section 7).
const CLIENT_SET: [&str; 3] = ["identityId", "emailId", "envelope"];

/// The creates that read by creation id, beside the refusal of each
/// other.
pub(super) fn creates(
    context: &Context<'_>,
    create: &Map<String, Value>,
) -> (Vec<(String, Submitting)>, Map<String, Value>) {
    let mut read = Vec::new();
    let mut refused = Map::new();
    for (id, object) in create {
        match submitting(context, object) {
            Ok(submitting) => read.push((id.clone(), submitting)),
            Err(error) => {
                refused.insert(id.clone(), error.object());
            }
        }
    }
    (read, refused)
}

fn submitting(context: &Context<'_>, object: &Value) -> Result<Submitting, SubmitError> {
    let invalid = |name: &str| SubmitError::InvalidProperties(vec![name.to_owned()]);
    let Some(object) = object.as_object() else {
        return Err(SubmitError::InvalidProperties(Vec::new()));
    };
    let foreign: Vec<String> = object
        .keys()
        .filter(|name| !CLIENT_SET.contains(&name.as_str()))
        .cloned()
        .collect();
    if !foreign.is_empty() {
        return Err(SubmitError::InvalidProperties(foreign));
    }
    let identity = object.get("identityId").and_then(Value::as_str);
    if identity != Some(context.key.as_str()) {
        return Err(invalid("identityId"));
    }
    let email = object
        .get("emailId")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("emailId"))?;
    // A creation id names an email the request makes before this call;
    // the send resolves it.
    if !email.starts_with('#') && !held(context, email)? {
        return Err(invalid("emailId"));
    }
    let envelope = match object.get("envelope") {
        None | Some(Value::Null) => None,
        Some(value) => Some(envelope(context, value)?),
    };
    Ok(Submitting {
        email: email.to_owned(),
        envelope,
    })
}

/// Whether the account holds the email.
fn held(context: &Context<'_>, email: &str) -> Result<bool, SubmitError> {
    let snapshot = context
        .store
        .emails(context.key, &[email])
        .map_err(|_| SubmitError::ServerFail)?;
    Ok(!snapshot.rows.is_empty())
}

/// The recipients of the envelope the call named: its sender is the
/// identity's address, it names at least one recipient and no parameter
/// on either, since the bridge advertises no SMTP extension.
fn envelope(context: &Context<'_>, value: &Value) -> Result<Envelope, SubmitError> {
    let invalid = || SubmitError::InvalidProperties(vec!["envelope".to_owned()]);
    let object = value.as_object().ok_or_else(invalid)?;
    if object
        .keys()
        .any(|name| name != "mailFrom" && name != "rcptTo")
    {
        return Err(invalid());
    }
    let mail_from = object
        .get("mailFrom")
        .and_then(address)
        .ok_or_else(invalid)?;
    let listed = object
        .get("rcptTo")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let rcpt_to: Vec<String> = listed
        .iter()
        .map(address)
        .collect::<Option<_>>()
        .ok_or_else(invalid)?;
    if !mail_from.eq_ignore_ascii_case(context.address) {
        return Err(SubmitError::ForbiddenMailFrom);
    }
    if rcpt_to.is_empty() {
        return Err(SubmitError::NoRecipients);
    }
    Ok(Envelope { rcpt_to })
}

/// One Address object whose `parameters` is null, absent or an empty object.
fn address(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if object
        .keys()
        .any(|name| name != "email" && name != "parameters")
    {
        return None;
    }
    let parameters = object.get("parameters").unwrap_or(&Value::Null);
    if !parameters.is_null() && !parameters.as_object().is_some_and(Map::is_empty) {
        return None;
    }
    object.get("email")?.as_str().map(str::to_owned)
}

/// What each creation id of the call leads to once it went out. A key
/// that names a submission of an earlier call names nothing this call
/// sends, so it does nothing; a patch that is no object is
/// `invalidArguments`.
pub(super) fn on_success(
    update: Map<String, Value>,
    destroy: Vec<String>,
) -> Result<Vec<(String, OnSuccess)>, MethodError> {
    let mut actions = Vec::new();
    for (key, patch) in update {
        let Some(creation) = key.strip_prefix('#') else {
            continue;
        };
        if !patch.is_object() {
            return Err(MethodError::InvalidArguments(
                "an onSuccessUpdateEmail patch is not an object",
            ));
        }
        actions.push((creation.to_owned(), OnSuccess::Update(patch)));
    }
    for key in destroy {
        if let Some(creation) = key.strip_prefix('#') {
            actions.push((creation.to_owned(), OnSuccess::Destroy));
        }
    }
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::address;

    #[test]
    fn an_address_names_no_parameter_as_null_absent_or_an_empty_object_rfc8621_7() {
        let email = "li@example.test";
        let none = [
            json!({ "email": email }),
            json!({ "email": email, "parameters": null }),
            json!({ "email": email, "parameters": {} }),
        ];
        for value in none {
            assert_eq!(address(&value).as_deref(), Some(email), "{value}");
        }
        let named = [
            json!({ "email": email, "parameters": { "SIZE": "1" } }),
            json!({ "email": email, "parameters": "SIZE=1" }),
            json!({ "email": email, "name": "Li" }),
        ];
        for value in named {
            assert_eq!(address(&value), None, "{value}");
        }
    }
}
