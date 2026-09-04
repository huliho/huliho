// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "zod";

// Zod probes for eval unless told not to; a strict CSP reports that probe.
// The flag is read when a schema is built, so every schema takes z from here.
z.config({ jitless: true });

export { z };
