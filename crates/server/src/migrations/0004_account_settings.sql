-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

-- The address the list shows, a display name, the preset the account was
-- added under and its connection settings as JSON. The credential stays
-- in the sealed column. Rows from before read as generic with no settings.
ALTER TABLE accounts ADD COLUMN address TEXT NOT NULL DEFAULT '';
ALTER TABLE accounts ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'generic'
    CHECK (provider IN ('gmail', 'microsoft', 'fastmail', 'icloud', 'yahoo', 'generic'));
ALTER TABLE accounts ADD COLUMN settings TEXT NOT NULL DEFAULT '{}';
