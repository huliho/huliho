-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

-- A user without a hash cannot sign in locally; external identities stay NULL.
ALTER TABLE users ADD COLUMN password_hash TEXT;

-- token_hash is the digest of the opaque cookie token; sealed holds the
-- authoritative session data under AEAD with the token hash as its AAD.
CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id),
    sealed BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);
