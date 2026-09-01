-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

CREATE TABLE organizations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organizations (id),
    login TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    external_issuer TEXT,
    external_subject TEXT,
    created_at INTEGER NOT NULL,
    -- An external identity is an issuer plus subject pair, set together.
    CHECK ((external_issuer IS NULL) = (external_subject IS NULL)),
    UNIQUE (external_issuer, external_subject)
) STRICT;

CREATE INDEX users_organization ON users (organization_id);

CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organizations (id),
    user_id TEXT NOT NULL REFERENCES users (id),
    kind TEXT NOT NULL CHECK (kind IN ('jmap', 'imap')),
    auth_method TEXT NOT NULL CHECK (auth_method IN ('password', 'bearer', 'oauth2')),
    credentials BLOB,
    stopped_cause TEXT,
    stopped_at INTEGER,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX accounts_user ON accounts (user_id);

CREATE TABLE auth_providers (
    id TEXT PRIMARY KEY,
    issuer TEXT NOT NULL UNIQUE,
    discovery_url TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret BLOB,
    created_at INTEGER NOT NULL
) STRICT;

-- No foreign key on purpose: event rows outlive the entities they describe.
CREATE TABLE domain_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    event_type TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX domain_events_organization ON domain_events (organization_id);

CREATE INDEX domain_events_created_at ON domain_events (created_at);

CREATE TRIGGER domain_events_append_only
BEFORE UPDATE ON domain_events
BEGIN
    SELECT RAISE(ABORT, 'domain_events is append-only');
END;

CREATE TABLE user_preferences (
    user_id TEXT NOT NULL REFERENCES users (id),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, key)
) STRICT;

CREATE TABLE sender_policies (
    user_id TEXT NOT NULL REFERENCES users (id),
    sender TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, sender, key)
) STRICT;

CREATE TABLE snooze_schedule (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    email_id TEXT NOT NULL,
    wake_at INTEGER NOT NULL,
    return_mailbox_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, email_id)
) STRICT;
