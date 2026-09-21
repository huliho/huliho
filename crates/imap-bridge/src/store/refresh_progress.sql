-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

CREATE INDEX bridge_message_ids_thread ON bridge_message_ids (account_key, thread_id);

ALTER TABLE bridge_sync ADD COLUMN uid_next INTEGER;
ALTER TABLE bridge_sync ADD COLUMN highest_modseq INTEGER;
ALTER TABLE bridge_sync ADD COLUMN messages INTEGER;
