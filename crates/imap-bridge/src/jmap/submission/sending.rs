// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What an `EmailSubmission/set` asked, done between two passes over the
//! request, one send of the account at a time: the message fetched whole
//! on the account's conversation, which it then lets go; its sender and
//! recipients read and its two forms written off the runtime; the wire
//! form sent over a connection of its own; the send recorded; then the
//! implicit `Email/set` for what the call named on success.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::time::timeout;

use super::implicit::{Account, Implicit, SentForm};
use super::{OnSuccess, SubmitAsk, SubmitError, Submitted, Submitting};
use crate::blob::{self, BlobError};
use crate::mailboxes::SyncError;
use crate::message::{Forms, MAX_MESSAGE_BYTES, Unsendable, recipients, sendable};
use crate::runtime::folders::{Round, creation_id};
use crate::runtime::{ConnectError, Connector, Link};
use crate::session::SessionError;
use crate::smtp::SendError;
use crate::store::{EmailId, SubmissionId, SubmissionRow};
use crate::sync::{Cache, blocking};

/// Sends every create of the ask in its order and answers what came of
/// each, with the implicit call where the call asked for one.
///
/// # Errors
///
/// Returns the store's failure or `Task` from before any message went
/// out.
pub(in crate::jmap) async fn apply<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    ask: &SubmitAsk,
    round: &mut Round<'_>,
) -> Result<Submitted, SyncError> {
    let old_state = cache.store.submissions(&cache.key).state;
    let mut created = Vec::new();
    let mut implicit = Implicit::default();
    let mut went_out = false;
    for (creation, submitting) in &ask.creates {
        let _turn = link.sends.turn().await;
        let email = creation_id(&submitting.email)
            .and_then(|named| round.created.get(named))
            .map_or_else(|| submitting.email.clone(), Clone::clone);
        let email = EmailId::from(email);
        let outcome = judged(sent(cache, link, &email, submitting).await, went_out)?;
        let outcome = match outcome {
            Ok((submission, form)) => {
                went_out = true;
                cache
                    .store
                    .record_submission(&cache.key, submission.clone());
                round
                    .created
                    .insert(creation.clone(), submission.id.to_string());
                if let Some(action) = on_success(ask, creation) {
                    let account = Account {
                        cache,
                        link,
                        created: &*round.created,
                    };
                    implicit.run(&account, form, action).await;
                }
                Ok(submission)
            }
            Err(error) => Err(error),
        };
        created.push((creation.clone(), outcome));
    }
    Ok(Submitted {
        old_state,
        new_state: cache.store.submissions(&cache.key).state,
        created,
        implicit: ask.asked.then_some(implicit),
    })
}

/// What the call named for a create on success; a destroy wins over an
/// update of the same email.
fn on_success<'a>(ask: &'a SubmitAsk, creation: &str) -> Option<&'a OnSuccess> {
    let named = || (ask.on_success.iter()).filter(|(named, _)| named == creation);
    let destroy = named().find(|(_, action)| *action == OnSuccess::Destroy);
    destroy.or_else(|| named().next()).map(|(_, action)| action)
}

/// What one send came to: the record of the send with the Sent form of
/// the message, else why nothing went out.
type Sent = Result<(SubmissionRow, SentForm), SubmitError>;

/// What one create answers given what its send came to. A failure of
/// the store or of the task is the request's before any message went
/// out; after one went out it answers for the create alone, since a
/// client keeps a request without a verdict queued and would send that
/// message again.
fn judged(sent: Result<Sent, SyncError>, went_out: bool) -> Result<Sent, SyncError> {
    match sent {
        Err(_) if went_out => Ok(Err(SubmitError::ServerFail)),
        other => other,
    }
}

async fn sent<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    email: &EmailId,
    submitting: &Submitting,
) -> Result<Sent, SyncError> {
    let store = Arc::clone(&cache.store);
    let (key, id) = (cache.key.clone(), email.clone());
    let rows = blocking(move || store.emails(&key, &[id.as_str()])).await?;
    let Some(row) = rows.rows.into_iter().next() else {
        return Ok(Err(SubmitError::InvalidProperties(vec![
            "emailId".to_owned(),
        ])));
    };
    let message = match fetched(cache, link, email).await {
        Ok(message) => message,
        Err(error) => return Ok(Err(error)),
    };
    let now = now();
    let identity = cache.address.clone();
    let given = submitting.envelope.as_ref().map(|it| it.rcpt_to.clone());
    let read = tokio::task::spawn_blocking(move || -> Result<(Vec<String>, Forms), Unsendable> {
        let rcpt_to = recipients(&message, &identity, given.as_deref())?;
        Ok((rcpt_to, sendable(&message, now)?))
    });
    let (rcpt_to, forms) = match read.await.map_err(|_join| SyncError::Task)? {
        Ok(read) => read,
        Err(unsendable) => return Ok(Err(refused(unsendable))),
    };
    let connection = match link.sends.connect(&cache.key).await {
        Ok(connection) => connection,
        Err(error) => return Ok(Err(unconnected(&error))),
    };
    if let Err(error) = connection.send(&cache.address, &rcpt_to, &forms.wire).await {
        return Ok(Err(undelivered(error)));
    }
    let submission = SubmissionRow {
        id: SubmissionId::generate(),
        identity_id: cache.key.as_str().to_owned(),
        email_id: email.clone(),
        thread_id: row.thread_id.clone(),
        mail_from: cache.address.clone(),
        rcpt_to,
        send_at: now,
    };
    let form = SentForm {
        row,
        bytes: forms.sent,
        date: now,
    };
    Ok(Ok((submission, form)))
}

/// The message whole on the account's conversation within its deadline,
/// under the bound a draft is written to.
async fn fetched<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    email: &EmailId,
) -> Result<Vec<u8>, SubmitError> {
    let mut wire = link.wire.lock().await;
    let limit = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX);
    let deadline = wire.deadline();
    let read = timeout(deadline, async {
        let session = wire.session(cache).await.map_err(|_| None)?;
        blob::whole(session, cache, email.as_str(), limit)
            .await
            .map_err(Some)
    })
    .await;
    match read {
        Ok(Ok(message)) => Ok(message),
        Ok(Err(None)) => Err(SubmitError::ServerUnavailable),
        Ok(Err(Some(stop))) => {
            if !stop.stands {
                wire.drop_session();
            }
            Err(match stop.error {
                BlobError::NotFound => SubmitError::InvalidProperties(vec!["emailId".to_owned()]),
                BlobError::TooLarge => SubmitError::TooLarge(limit),
                BlobError::Unavailable => SubmitError::ServerUnavailable,
                BlobError::Undecodable | BlobError::Store(_) | BlobError::Task => {
                    SubmitError::ServerFail
                }
            })
        }
        Err(_elapsed) => {
            wire.drop_session();
            Err(SubmitError::ServerUnavailable)
        }
    }
}

/// Seconds since the epoch.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or_default()
}

fn refused(unsendable: Unsendable) -> SubmitError {
    match unsendable {
        Unsendable::InvalidEmail(property) => SubmitError::InvalidEmail(property),
        Unsendable::ForbiddenFrom => SubmitError::ForbiddenFrom,
        Unsendable::NoRecipients => SubmitError::NoRecipients,
        Unsendable::InvalidRecipients(addresses) => SubmitError::InvalidRecipients(addresses),
    }
}

/// A connection that did not come about: the credential the server
/// refused, since a send waiting for it would wait for good; an account
/// the host holds back or a server out of reach, which a later round may
/// reach; any other failure the server's, which waiting does not mend.
fn unconnected(error: &ConnectError) -> SubmitError {
    match error {
        ConnectError::Failed(SessionError::CredentialRejected) => SubmitError::ForbiddenToSend,
        ConnectError::Held
        | ConnectError::Failed(
            SessionError::NoAddress
            | SessionError::Connect(_)
            | SessionError::Timeout
            | SessionError::Closed
            | SessionError::Io(_)
            | SessionError::Unavailable,
        ) => SubmitError::ServerUnavailable,
        ConnectError::Failed(_) => SubmitError::ServerFail,
    }
}

fn undelivered(error: SendError) -> SubmitError {
    match error {
        SendError::InvalidRecipients(addresses) => SubmitError::InvalidRecipients(addresses),
        SendError::TooManyRecipients(most) => SubmitError::TooManyRecipients(most),
        SendError::ForbiddenMailFrom => SubmitError::ForbiddenMailFrom,
        SendError::TooLarge(most) => SubmitError::TooLarge(most),
        SendError::Unavailable => SubmitError::ServerUnavailable,
        SendError::Refused => SubmitError::ServerFail,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::store::{EmailRow, StoreError, ThreadId};

    /// A send that went out: its record and the Sent form.
    fn went() -> (SubmissionRow, SentForm) {
        let row = EmailRow {
            id: EmailId::generate(),
            thread_id: ThreadId::generate(),
            mailbox_ids: Vec::new(),
            keywords: BTreeMap::new(),
            size: 0,
            received_at: 0,
            sent_at: None,
            has_attachment: false,
            sealed: Vec::new(),
        };
        let submission = SubmissionRow {
            id: SubmissionId::generate(),
            identity_id: "a1".to_owned(),
            email_id: row.id.clone(),
            thread_id: row.thread_id.clone(),
            mail_from: "sanne@example.test".to_owned(),
            rcpt_to: vec!["mo@example.test".to_owned()],
            send_at: 0,
        };
        let form = SentForm {
            row,
            bytes: Vec::new(),
            date: 0,
        };
        (submission, form)
    }

    #[test]
    fn a_failure_is_the_requests_before_a_send_and_the_creates_own_after_one() {
        let store = || SyncError::Store(StoreError::Poisoned);
        assert!(matches!(
            judged(Err(SyncError::Task), false),
            Err(SyncError::Task)
        ));
        assert!(matches!(
            judged(Err(store()), false),
            Err(SyncError::Store(StoreError::Poisoned))
        ));
        let failed = Err(SubmitError::ServerFail);
        assert_eq!(judged(Err(SyncError::Task), true).unwrap(), failed);
        assert_eq!(judged(Err(store()), true).unwrap(), failed);
        for went_out in [false, true] {
            let refused = judged(Ok(Err(SubmitError::NoRecipients)), went_out);
            assert_eq!(refused.unwrap(), Err(SubmitError::NoRecipients));
            let pair = went();
            let passed = judged(Ok(Ok(pair.clone())), went_out);
            assert_eq!(passed.unwrap(), Ok(pair));
        }
    }
}
