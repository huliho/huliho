// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Account rows: adding with a sealed credential, listing, reading and
//! removal under the scope rules.

mod connecting;
mod organization;

use connecting::{ACCOUNT_TOKEN, keys, new_account};
use huliho_server::accounts::{
    self, AccountKind, AccountSettings, AuthMethod, Credential, Endpoint, NewAccount, Provider,
    TlsMode,
};
use huliho_server::identity;
use huliho_server::ids::{AccountId, Role};
use huliho_server::scope;
use huliho_server::secrets::{InstanceSecret, Keys};
use huliho_server::store::{Store, StoreError};
use organization::{new_user, personal, scope_of, store};

const ADDRESS: &str = "mira@fastmail.com";
const IMAP_ADDRESS: &str = "sanne@example.net";
const IMAP_HOST: &str = "imap.example.net";
const SMTP_HOST: &str = "smtp.example.net";
const PASSWORD: &str = "app password 1234";

/// A generic IMAP account with a password.
fn imap_account(address: &str, password: &str) -> NewAccount {
    let endpoint = |host: &str, port: u16, tls: TlsMode| Endpoint {
        host: host.to_owned(),
        port,
        tls,
    };
    NewAccount {
        address: address.to_owned(),
        name: "Work".to_owned(),
        provider: Provider::Generic,
        settings: AccountSettings::Imap {
            username: address.to_owned(),
            imap: endpoint(IMAP_HOST, 993, TlsMode::Implicit),
            smtp: endpoint(SMTP_HOST, 587, TlsMode::Starttls),
        },
        credential: Credential::Password {
            password: password.to_owned(),
        },
    }
}

/// A store on disk, so a second connection can look at the raw rows.
fn store_on_disk() -> (tempfile::TempDir, Store, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Store::open(&data).unwrap();
    let database = rusqlite::Connection::open(data.join("huliho.db")).unwrap();
    (dir, store, database)
}

fn column<T: rusqlite::types::FromSql>(
    database: &rusqlite::Connection,
    column: &str,
    id: &AccountId,
) -> T {
    database
        .query_row(
            &format!("SELECT {column} FROM accounts WHERE id = ?1"),
            [id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn an_account_never_resolves_for_another_user() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (_, beta) = personal(&store, "beta@example.com");
    let account = accounts::add(
        &store,
        &keys(),
        &scope_of(&store, &alpha),
        &new_account(ADDRESS),
    )
    .unwrap();
    let result = scope::resolve(&store, &beta.id, Some(&account.id));
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn a_listing_stays_inside_the_own_scope() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (_, beta) = personal(&store, "beta@example.com");
    accounts::add(
        &store,
        &keys(),
        &scope_of(&store, &alpha),
        &new_account(ADDRESS),
    )
    .unwrap();
    assert!(
        accounts::list(&store, &scope_of(&store, &beta))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_admin_of_the_same_organization_sees_no_member_accounts() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let member = identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();
    let account = accounts::add(
        &store,
        &keys(),
        &scope_of(&store, &member),
        &new_account(ADDRESS),
    )
    .unwrap();
    assert!(accounts::list(&store, &owner_scope).unwrap().is_empty());
    let result = scope::resolve(&store, &owner.id, Some(&account.id));
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn resolving_an_unknown_account_is_not_found() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let unknown = AccountId::from("unknown".to_owned());
    let result = scope::resolve(&store, &user.id, Some(&unknown));
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn a_read_needs_an_account_scope() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    assert!(matches!(
        accounts::get(&store, &scope),
        Err(StoreError::MissingAccount)
    ));
    assert!(matches!(
        accounts::credential(&store, &keys(), &scope),
        Err(StoreError::MissingAccount)
    ));
}

#[test]
fn another_instance_secret_opens_no_credential() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let account = accounts::add(
        &store,
        &keys(),
        &scope_of(&store, &user),
        &new_account(ADDRESS),
    )
    .unwrap();
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    let other = Keys::derive(
        &InstanceSecret::from_bytes(b"fedcba9876543210fedcba9876543210".to_vec()).unwrap(),
    );
    let result = accounts::credential(&store, &other, &scoped);
    assert!(matches!(result, Err(StoreError::Tampered)));
}

#[test]
fn a_credential_blob_moved_to_another_row_does_not_open() {
    let (_dir, store, database) = store_on_disk();
    let (_, user) = personal(&store, "mira@example.com");
    let keys = keys();
    let scope = scope_of(&store, &user);
    let first = accounts::add(&store, &keys, &scope, &new_account(ADDRESS)).unwrap();
    let second =
        accounts::add(&store, &keys, &scope, &imap_account(IMAP_ADDRESS, PASSWORD)).unwrap();
    database
        .execute(
            "UPDATE accounts SET credentials = (SELECT credentials FROM accounts WHERE id = ?1)
             WHERE id = ?2",
            [first.id.as_str(), second.id.as_str()],
        )
        .unwrap();
    let moved = scope::resolve(&store, &user.id, Some(&second.id)).unwrap();
    let result = accounts::credential(&store, &keys, &moved);
    assert!(matches!(result, Err(StoreError::Tampered)));
    let kept = scope::resolve(&store, &user.id, Some(&first.id)).unwrap();
    assert!(accounts::credential(&store, &keys, &kept).is_ok());
}

#[test]
fn the_row_holds_no_plaintext_credential() {
    let (_dir, store, database) = store_on_disk();
    let (_, user) = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let jmap = accounts::add(&store, &keys(), &scope, &new_account(ADDRESS)).unwrap();
    let imap = accounts::add(
        &store,
        &keys(),
        &scope,
        &imap_account(IMAP_ADDRESS, PASSWORD),
    )
    .unwrap();
    let sealed_token: Vec<u8> = column(&database, "credentials", &jmap.id);
    assert!(!contains(&sealed_token, ACCOUNT_TOKEN));
    let sealed_password: Vec<u8> = column(&database, "credentials", &imap.id);
    assert!(!contains(&sealed_password, PASSWORD));
    let settings: String = column(&database, "settings", &imap.id);
    assert!(!settings.contains(PASSWORD));
    assert!(settings.contains(IMAP_HOST));
}

#[test]
fn removing_an_account_takes_its_credential_along() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let keys = keys();
    let scope = scope_of(&store, &user);
    let account = accounts::add(&store, &keys, &scope, &new_account(ADDRESS)).unwrap();
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    accounts::remove(&store, &scoped).unwrap();
    assert!(accounts::list(&store, &scope).unwrap().is_empty());
    let resolved = scope::resolve(&store, &user.id, Some(&account.id));
    assert!(matches!(resolved, Err(StoreError::NotFound)));
    let repeated = accounts::remove(&store, &scoped);
    assert!(matches!(repeated, Err(StoreError::NotFound)));
    let gone = accounts::credential(&store, &keys, &scoped);
    assert!(matches!(gone, Err(StoreError::NotFound)));
}

#[test]
fn adding_lists_and_reads_within_scope() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let account = accounts::add(&store, &keys(), &scope, &new_account(ADDRESS)).unwrap();
    let listed = accounts::list(&store, &scope).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, account.id);
    assert_eq!(listed[0].address, ADDRESS);
    assert_eq!(listed[0].name, "Fastmail");
    assert_eq!(listed[0].provider, Provider::Fastmail);
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    let read = accounts::get(&store, &scoped).unwrap();
    assert_eq!(read.id, account.id);
    assert_eq!(read.kind, AccountKind::Jmap);
    assert_eq!(read.auth_method, AuthMethod::Bearer);
    assert!(read.stopped_cause.is_none());
    assert!(read.stopped_at.is_none());
}

#[test]
fn the_credential_round_trips_through_its_row() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    let keys = keys();
    let scope = scope_of(&store, &user);
    let new = imap_account(IMAP_ADDRESS, PASSWORD);
    let account = accounts::add(&store, &keys, &scope, &new).unwrap();
    assert_eq!(account.kind, AccountKind::Imap);
    assert_eq!(account.auth_method, AuthMethod::Password);
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    assert_eq!(
        accounts::credential(&store, &keys, &scoped).unwrap(),
        new.credential
    );
}
