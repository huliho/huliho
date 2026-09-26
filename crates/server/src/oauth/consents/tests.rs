// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The consent ledger: start, claim, settle, end and expiry per owner.

use super::*;

const NOW: i64 = 1_000_000;

fn user(name: &str) -> UserId {
    UserId::from(name.to_owned())
}

fn account(id: &str) -> AccountId {
    AccountId::from(id.to_owned())
}

fn filed(consents: &Consents, state: &str, name: &str, now: i64) {
    consents.insert(
        NewConsent {
            state: state.to_owned(),
            verifier: format!("verifier-{state}"),
            user_id: user(name),
            provider: OauthProvider::Google,
            account_provider: Provider::Gmail,
            address: Address::parse("sanne@gmail.com").unwrap(),
            account_id: None,
        },
        now,
    );
}

fn claim(consents: &Consents, name: &str, provider: OauthProvider, state: &str) -> Option<Claimed> {
    consents.claim(
        Claimant {
            user_id: &user(name),
            provider,
        },
        state,
        NOW,
    )
}

#[test]
fn a_consent_is_pending_for_its_owner_and_absent_for_anyone_else() {
    let consents = Consents::default();
    filed(&consents, "s1", "mira", NOW);
    assert_eq!(
        consents.outcome(&user("mira"), "s1", NOW),
        Some(Outcome::Pending)
    );
    assert_eq!(consents.outcome(&user("noor"), "s1", NOW), None);
    assert_eq!(consents.outcome(&user("mira"), "s2", NOW), None);
}

#[test]
fn a_claim_happens_once_for_the_owner_and_the_provider() {
    let consents = Consents::default();
    filed(&consents, "s1", "mira", NOW);
    assert!(claim(&consents, "noor", OauthProvider::Google, "s1").is_none());
    assert!(claim(&consents, "mira", OauthProvider::Microsoft, "s1").is_none());
    let claimed = claim(&consents, "mira", OauthProvider::Google, "s1").unwrap();
    assert_eq!(claimed.verifier, "verifier-s1");
    assert_eq!(claimed.provider, OauthProvider::Google);
    assert_eq!(claimed.account_provider, Provider::Gmail);
    assert_eq!(claimed.address.to_string(), "sanne@gmail.com");
    assert!(claim(&consents, "mira", OauthProvider::Google, "s1").is_none());
    assert_eq!(
        consents.outcome(&user("mira"), "s1", NOW),
        Some(Outcome::Pending)
    );
    consents.settle("s1", Ok(account("a")));
    assert_eq!(
        consents.outcome(&user("mira"), "s1", NOW),
        Some(Outcome::Done {
            account_id: account("a")
        })
    );
}

#[test]
fn a_denied_consent_names_its_cause() {
    let consents = Consents::default();
    filed(&consents, "s1", "mira", NOW);
    claim(&consents, "mira", OauthProvider::Google, "s1").unwrap();
    consents.settle("s1", Err(DeniedCause::SmtpAuthUnavailable));
    assert_eq!(
        consents.outcome(&user("mira"), "s1", NOW),
        Some(Outcome::Denied {
            cause: DeniedCause::SmtpAuthUnavailable
        })
    );
}

#[test]
fn an_open_consent_ends_for_its_owner_alone_and_a_claimed_or_settled_one_stays() {
    let consents = Consents::default();
    filed(&consents, "s1", "mira", NOW);
    assert!(!consents.end(&user("noor"), "s1", NOW));
    assert!(!consents.end(&user("mira"), "s2", NOW));
    assert!(!consents.end(&user("mira"), "s1", NOW + CONSENT_TTL_MS));
    assert!(consents.end(&user("mira"), "s1", NOW));
    assert_eq!(consents.outcome(&user("mira"), "s1", NOW), None);
    filed(&consents, "s2", "mira", NOW);
    claim(&consents, "mira", OauthProvider::Google, "s2").unwrap();
    assert!(!consents.end(&user("mira"), "s2", NOW));
    consents.settle("s2", Ok(account("a")));
    assert!(!consents.end(&user("mira"), "s2", NOW));
    assert_eq!(
        consents.outcome(&user("mira"), "s2", NOW),
        Some(Outcome::Done {
            account_id: account("a")
        })
    );
}

#[test]
fn a_consent_expires_after_ten_minutes_and_leaves_on_the_next_insert() {
    let consents = Consents::default();
    filed(&consents, "s1", "mira", NOW);
    let later = NOW + CONSENT_TTL_MS;
    assert_eq!(consents.outcome(&user("mira"), "s1", later), None);
    let claimant = Claimant {
        user_id: &user("mira"),
        provider: OauthProvider::Google,
    };
    assert!(consents.claim(claimant, "s1", later).is_none());
    filed(&consents, "s2", "mira", later);
    assert_eq!(consents.len(), 1);
}

#[test]
fn a_fifth_consent_evicts_the_users_settled_or_oldest_and_nobody_elses() {
    let consents = Consents::default();
    for (index, state) in ["a", "b", "c", "d"].into_iter().enumerate() {
        filed(
            &consents,
            state,
            "mira",
            NOW + i64::try_from(index).unwrap(),
        );
    }
    filed(&consents, "noor", "noor", NOW);
    filed(&consents, "e", "mira", NOW + 10);
    assert_eq!(consents.outcome(&user("mira"), "a", NOW + 10), None);
    for state in ["b", "c", "d", "e"] {
        assert_eq!(
            consents.outcome(&user("mira"), state, NOW + 10),
            Some(Outcome::Pending),
            "{state}"
        );
    }
    assert_eq!(
        consents.outcome(&user("noor"), "noor", NOW + 10),
        Some(Outcome::Pending)
    );
    assert_eq!(consents.len(), 5);
    consents.settle("c", Err(DeniedCause::AccessDenied));
    filed(&consents, "f", "mira", NOW + 11);
    assert_eq!(consents.outcome(&user("mira"), "c", NOW + 11), None);
    assert_eq!(
        consents.outcome(&user("mira"), "b", NOW + 11),
        Some(Outcome::Pending)
    );
}

#[test]
fn the_outcomes_serialize_to_their_words() {
    assert_eq!(
        serde_json::to_string(&Outcome::Pending).unwrap(),
        r#"{"status":"pending"}"#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Done {
            account_id: account("a")
        })
        .unwrap(),
        r#"{"status":"done","accountId":"a"}"#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Denied {
            cause: DeniedCause::NoRefreshToken
        })
        .unwrap(),
        r#"{"status":"denied","cause":"noRefreshToken"}"#
    );
}

#[test]
fn every_probe_error_has_one_denied_word() {
    let cases = [
        (
            ProbeError::CredentialRejected,
            DeniedCause::UpstreamCredentials,
        ),
        (
            ProbeError::Unreachable(String::new()),
            DeniedCause::UpstreamUnreachable,
        ),
        (
            ProbeError::Insecure(String::new()),
            DeniedCause::UpstreamInsecure,
        ),
        (
            ProbeError::Unsupported(String::new()),
            DeniedCause::UpstreamUnsupported,
        ),
        (
            ProbeError::SmtpAuthUnavailable,
            DeniedCause::SmtpAuthUnavailable,
        ),
    ];
    for (error, cause) in cases {
        assert_eq!(DeniedCause::from(error), cause);
    }
}
