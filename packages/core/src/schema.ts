// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "zod/mini";

// No schema built on zod/mini generates code or probes for eval, which a
// strict CSP would report; jitless keeps a later path that honors the flag
// from doing so. Every schema takes z from here, so the flag is set before
// one is built.
z.config({ jitless: true });

export { z };
