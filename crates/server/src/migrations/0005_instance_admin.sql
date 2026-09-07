-- Copyright (C) 2026 Eric Kochen
-- SPDX-License-Identifier: AGPL-3.0-only
-- Additional terms apply, see NOTICE.

-- An instance property beside the organization role: whoever holds it
-- writes the rows the whole instance shares. Nobody holds it at first.
ALTER TABLE users ADD COLUMN instance_admin INTEGER NOT NULL DEFAULT 0
    CHECK (instance_admin IN (0, 1));
