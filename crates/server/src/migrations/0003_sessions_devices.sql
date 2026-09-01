-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

-- The display name shown in the admin views; existing rows keep their login as the name.
ALTER TABLE users ADD COLUMN name TEXT NOT NULL DEFAULT '';
UPDATE users SET name = login;

ALTER TABLE users ADD COLUMN last_active_at INTEGER;

-- Set only while password_hash holds a one-time password.
ALTER TABLE users ADD COLUMN password_reset_expires_at INTEGER;

-- id is the public identifier, device a JSON record of the client and
-- address where the session was last used from. Existing rows keep their tokens.
CREATE TABLE sessions_next (
    token_hash BLOB PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    user_id TEXT NOT NULL REFERENCES users (id),
    sealed BLOB NOT NULL,
    device TEXT NOT NULL,
    address TEXT,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
) STRICT;

INSERT INTO sessions_next (token_hash, id, user_id, sealed, device, address, created_at, last_seen_at)
SELECT token_hash, lower(hex(randomblob(16))), user_id, sealed, '{}', NULL, created_at, last_seen_at
FROM sessions;

DROP TABLE sessions;

ALTER TABLE sessions_next RENAME TO sessions;

CREATE INDEX sessions_user ON sessions (user_id);
