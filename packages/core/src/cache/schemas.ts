// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { emailBodyPartSchema, emailBodyValueSchema } from "../jmap/body";
import { countSchema, idSchema, setSchema, threadSchema } from "../jmap/schemas";
import { z } from "../schema";
import type { EmailBody, PendingRow, QueryRow, ThreadRow } from "./store";

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

const verdictSchema = z.enum(["pass", "fail", "none", "unknown"]);

const authenticationSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("absent") }),
  z.object({ status: z.literal("unparseable") }),
  z.object({
    status: z.literal("parsed"),
    results: z.object({
      server: z.string().nullable(),
      spf: verdictSchema,
      dkim: verdictSchema,
      dmarc: verdictSchema,
      dmarcFrom: z.string().nullable(),
    }),
  }),
]);

export const emailBodySchema: z.ZodType<EmailBody> = z.object({
  id: idSchema,
  bodyStructure: emailBodyPartSchema,
  textBody: z.array(emailBodyPartSchema),
  htmlBody: z.array(emailBodyPartSchema),
  attachments: z.array(emailBodyPartSchema),
  bodyValues: z.record(z.string(), emailBodyValueSchema),
  authentication: authenticationSchema,
  flowed: z.object({ delSp: z.boolean() }).nullable(),
  large: z.boolean(),
  fetchedAt: z.number(),
  bytes: countSchema,
});

// A path names one keyword, as the code that reads a patch asks.
const patchSchema = z.record(z.string().regex(/^keywords\/./u), z.literal(true).nullable());

export const pendingRowSchema: z.ZodType<PendingRow> = z.object({
  seq: z.number().int().positive(),
  type: z.literal("Email"),
  id: idSchema,
  patch: patchSchema,
  inverse: patchSchema,
  sentAt: z.number().nullable(),
});
