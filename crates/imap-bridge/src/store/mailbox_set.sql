-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

ALTER TABLE bridge_mailboxes ADD COLUMN role_override TEXT;
ALTER TABLE bridge_mailboxes ADD COLUMN rights TEXT;
