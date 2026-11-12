// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { documentSchema } from "../compose/document";
import { emailBodyPartSchema, emailBodyValueSchema } from "../jmap/body";
import { countSchema, identitySchema, idSchema, setSchema, threadSchema } from "../jmap/schemas";
import { z } from "../schema";
import type { DraftRow, EmailBody, PendingRow, QueryRow, ThreadRow } from "./store";

// The rows a store keeps beyond the JMAP objects, checked when they come
// back from disk.
const memberStateSchema = z.object({ keywords: setSchema, mailboxIds: setSchema });

export const threadRowSchema: z.ZodMiniType<ThreadRow> = z.extend(threadSchema, {
  members: z.record(z.string(), memberStateSchema),
});

const pageSchema = z.object({ page: countSchema, ids: z.array(idSchema) });

const freshPageSchema = z.object({
  ids: z.array(idSchema),
  total: z.nullable(countSchema),
  queryState: z.string(),
});

export const queryRowSchema: z.ZodMiniType<QueryRow> = z.object({
  id: idSchema,
  queryState: z.string(),
  total: z.nullable(countSchema),
  pages: z.array(pageSchema),
  pending: z.array(idSchema),
  fresh: z.nullable(freshPageSchema),
});

const verdictSchema = z.enum(["pass", "fail", "none", "unknown"]);

const authenticationSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("absent") }),
  z.object({ status: z.literal("unparseable") }),
  z.object({
    status: z.literal("parsed"),
    results: z.object({
      server: z.nullable(z.string()),
      spf: verdictSchema,
      dkim: verdictSchema,
      dmarc: verdictSchema,
      dmarcFrom: z.nullable(z.string()),
    }),
  }),
]);

export const emailBodySchema: z.ZodMiniType<EmailBody> = z.object({
  id: idSchema,
  bodyStructure: emailBodyPartSchema,
  textBody: z.array(emailBodyPartSchema),
  htmlBody: z.array(emailBodyPartSchema),
  attachments: z.array(emailBodyPartSchema),
  bodyValues: z.record(z.string(), emailBodyValueSchema),
  authentication: authenticationSchema,
  flowed: z.nullable(z.object({ delSp: z.boolean() })),
  large: z.boolean(),
  fetchedAt: z.number(),
  bytes: countSchema,
});

// A path names one keyword or one mailbox, as the code that reads a
// patch asks.
const patchSchema = z.record(
  z.string().check(z.regex(/^(?:keywords|mailboxIds)\/./u)),
  z.nullable(z.literal(true)),
);

const recipientSchema = z.union([
  z.object({ name: z.nullable(z.string()), email: z.string() }),
  z.object({ text: z.string() }),
]);

const draftAttachmentSchema = z.object({
  blobId: idSchema,
  name: z.nullable(z.string()),
  type: z.string(),
  size: countSchema,
  cid: z.nullable(z.string()),
  disposition: z.enum(["attachment", "inline"]),
});

export const draftRowSchema: z.ZodMiniType<DraftRow> = z.object({
  id: idSchema,
  kind: z.enum(["new", "reply", "forward"]),
  parentId: z.nullable(idSchema),
  serverId: z.nullable(idSchema),
  threadId: z.nullable(idSchema),
  parts: z.record(idSchema, idSchema),
  state: z.enum(["draft", "queued", "sending"]),
  sendAfter: z.nullable(z.number()),
  changedAt: z.number(),
  revision: countSchema,
  savedRevision: z.nullable(countSchema),
  savedAt: z.nullable(z.number()),
  mode: z.enum(["rich", "plain"]),
  document: documentSchema,
  html: z.string(),
  text: z.string(),
  to: z.array(recipientSchema),
  cc: z.array(recipientSchema),
  bcc: z.array(recipientSchema),
  subject: z.string(),
  identity: identitySchema,
  inReplyTo: z.array(z.string()),
  references: z.array(z.string()),
  attachments: z.array(draftAttachmentSchema),
  failure: z.nullable(
    z.object({
      stage: z.enum(["save", "send"]),
      type: z.string(),
      recipients: z.array(z.string()),
    }),
  ),
});

export const pendingRowSchema: z.ZodMiniType<PendingRow> = z.object({
  seq: z.int().check(z.positive()),
  type: z.literal("Email"),
  id: idSchema,
  threadId: z.exactOptional(idSchema),
  patch: patchSchema,
  inverse: patchSchema,
  sentAt: z.nullable(z.number()),
  destroy: z.exactOptional(z.literal(true)),
  held: z.exactOptional(z.boolean()),
  lease: z.exactOptional(z.string()),
});
