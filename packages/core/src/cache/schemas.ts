// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { countSchema, idSchema, setSchema, threadSchema } from "../jmap/schemas";
import { z } from "../schema";
import type { QueryRow, ThreadRow } from "./store";

// The rows a store keeps beyond the JMAP objects, checked when they come
// back from disk.
const memberStateSchema = z.object({ keywords: setSchema, mailboxIds: setSchema });

export const threadRowSchema: z.ZodType<ThreadRow> = threadSchema.extend({
  members: z.record(z.string(), memberStateSchema),
});

const pageSchema = z.object({ page: countSchema, ids: z.array(idSchema) });

const freshPageSchema = z.object({
  ids: z.array(idSchema),
  total: countSchema.nullable(),
  queryState: z.string(),
});

export const queryRowSchema: z.ZodType<QueryRow> = z.object({
  id: idSchema,
  queryState: z.string(),
  total: countSchema.nullable(),
  pages: z.array(pageSchema),
  pending: z.array(idSchema),
  fresh: freshPageSchema.nullable(),
});
