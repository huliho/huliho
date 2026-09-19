-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

CREATE TABLE bridge_mailboxes (
    account_key TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    imap_name TEXT NOT NULL,
    parent_id TEXT,
    role TEXT,
    sort_order INTEGER NOT NULL,
    subscribed INTEGER NOT NULL,
    selectable INTEGER NOT NULL,
    store INTEGER NOT NULL,
    gmail_label TEXT,
    uid_validity INTEGER,
    uid_next INTEGER,
    highest_modseq INTEGER,
    total_emails INTEGER NOT NULL DEFAULT 0,
    unread_emails INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_key, id),
    UNIQUE (account_key, imap_name)
) STRICT;

CREATE TABLE bridge_emails (
    account_key TEXT NOT NULL,
    id TEXT NOT NULL,
    folder_id TEXT NOT NULL,
    uid INTEGER NOT NULL,
    thread_id TEXT NOT NULL,
    keywords TEXT NOT NULL,
    size INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    sent_at INTEGER,
    has_attachment INTEGER NOT NULL,
    message_id_hash BLOB,
    gmail_msgid INTEGER,
    sealed BLOB NOT NULL,
    PRIMARY KEY (account_key, id),
    UNIQUE (account_key, folder_id, uid)
) STRICT;

CREATE INDEX bridge_emails_thread ON bridge_emails (account_key, thread_id);

CREATE UNIQUE INDEX bridge_emails_gmail
    ON bridge_emails (account_key, gmail_msgid)
    WHERE gmail_msgid IS NOT NULL;

CREATE TABLE bridge_memberships (
    account_key TEXT NOT NULL,
    email_id TEXT NOT NULL,
    mailbox_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (account_key, email_id, mailbox_id)
) STRICT;

CREATE INDEX bridge_memberships_window
    ON bridge_memberships (account_key, mailbox_id, received_at DESC, email_id);

CREATE TABLE bridge_message_ids (
    account_key TEXT NOT NULL,
    message_id_hash BLOB NOT NULL,
    thread_id TEXT NOT NULL,
    PRIMARY KEY (account_key, message_id_hash)
) STRICT;

CREATE TABLE bridge_sync (
    account_key TEXT NOT NULL,
    folder_id TEXT NOT NULL,
    lowest_synced_uid INTEGER,
    done INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_key, folder_id)
) STRICT;

CREATE TABLE bridge_state (
    account_key TEXT PRIMARY KEY,
    sequence INTEGER NOT NULL
) STRICT;

CREATE TABLE bridge_changes (
    account_key TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('Mailbox', 'Email', 'Thread')),
    id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('created', 'updated', 'destroyed')),
    PRIMARY KEY (account_key, sequence, type, id)
) STRICT;
