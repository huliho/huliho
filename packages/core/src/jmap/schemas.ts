// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "../schema";

// The core capability every server carries (RFC 8620 section 2).
export const CORE_CAPABILITY = "urn:ietf:params:jmap:core";

// The mail capability (RFC 8621 section 1.1).
export const MAIL_CAPABILITY = "urn:ietf:params:jmap:mail";

// The quota capability (RFC 9425 section 2.1).
export const QUOTA_CAPABILITY = "urn:ietf:params:jmap:quota";

// The vendor capability a bridge account advertises; it carries the
// Mailbox property syncedEmails.
export const HULIHO_CAPABILITY = "https://huliho.com/jmap";

// The properties of RFC 8621 section 4.1 the list and the reading pane
// render; bodies are not among them.
export const HEADER_PROPERTIES = [
  "id",
  "blobId",
  "threadId",
  "mailboxIds",
  "keywords",
  "size",
  "receivedAt",
  "messageId",
  "inReplyTo",
  "references",
  "sender",
  "from",
  "to",
  "cc",
  "bcc",
  "replyTo",
  "subject",
  "sentAt",
  "hasAttachment",
  "preview",
] as const;

// The two properties an email changes once it exists (RFC 8621 section
// 4.1.1) plus the thread that places it.
export const STATE_PROPERTIES = ["id", "threadId", "mailboxIds", "keywords"] as const;

// A path on this origin. The proxy rewrites every URL of the session
// object to one, so a call never leaves the instance.
const SAME_ORIGIN_PATH = /^\/(?![/\\])/;

export const idSchema = z.string().check(z.minLength(1));
export const countSchema = z.int().check(z.nonnegative());
// A set of ids or keywords as JMAP writes it: every value true.
export const setSchema = z.record(z.string(), z.literal(true));
// RFC 8620 section 1.4: a UTCDate ends in Z, a Date may carry an offset.
const utcDateSchema = z.iso.datetime();
const dateSchema = z.iso.datetime({ offset: true });

// The core limits the client keeps to (RFC 8620 section 2).
export const coreLimitsSchema = z.object({
  maxCallsInRequest: z.int().check(z.positive()),
  maxObjectsInGet: z.int().check(z.positive()),
  maxObjectsInSet: z.int().check(z.nonnegative()),
});

// The variables a download template carries (RFC 8620 section 2).
const DOWNLOAD_VARIABLES = ["{accountId}", "{blobId}", "{name}", "{type}"] as const;

const downloadUrlSchema = z.string().check(
  z.regex(SAME_ORIGIN_PATH),
  z.refine((template) => DOWNLOAD_VARIABLES.every((variable) => template.includes(variable))),
);

// An account without its capabilities reads as one that has none beyond
// what the session names.
const accountSchema = z.object({
  isReadOnly: z.boolean(),
  accountCapabilities: z.optional(z.record(z.string(), z.unknown())),
});

export const sessionObjectSchema = z.object({
  capabilities: z.record(z.string(), z.unknown()),
  accounts: z.record(z.string(), accountSchema),
  primaryAccounts: z.record(z.string(), idSchema),
  apiUrl: z.string().check(z.regex(SAME_ORIGIN_PATH)),
  downloadUrl: downloadUrlSchema,
  state: z.string(),
});

const rightsSchema = z.object({
  mayReadItems: z.boolean(),
  mayAddItems: z.boolean(),
  mayRemoveItems: z.boolean(),
  maySetSeen: z.boolean(),
  maySetKeywords: z.boolean(),
  mayCreateChild: z.boolean(),
  mayRename: z.boolean(),
  mayDelete: z.boolean(),
  maySubmit: z.boolean(),
});

export const mailboxSchema = z.object({
  id: idSchema,
  name: z.string(),
  parentId: z.nullable(idSchema),
  role: z.nullable(z.string()),
  sortOrder: countSchema,
  totalEmails: countSchema,
  unreadEmails: countSchema,
  totalThreads: countSchema,
  unreadThreads: countSchema,
  myRights: rightsSchema,
  isSubscribed: z.boolean(),
  // Under the vendor capability: the emails the bridge holds for the mailbox.
  syncedEmails: z.optional(countSchema),
});

const addressSchema = z.object({ name: z.nullable(z.string()), email: z.string() });
const addressesSchema = z.nullable(z.array(addressSchema));
const messageIdsSchema = z.nullable(z.array(z.string()));

export const emailStateSchema = z.object({
  id: idSchema,
  threadId: idSchema,
  mailboxIds: setSchema,
  keywords: setSchema,
});

export const emailHeaderSchema = z.extend(emailStateSchema, {
  blobId: z.string(),
  size: countSchema,
  receivedAt: utcDateSchema,
  messageId: messageIdsSchema,
  inReplyTo: messageIdsSchema,
  references: messageIdsSchema,
  sender: addressesSchema,
  from: addressesSchema,
  to: addressesSchema,
  cc: addressesSchema,
  bcc: addressesSchema,
  replyTo: addressesSchema,
  subject: z.nullable(z.string()),
  sentAt: z.nullable(dateSchema),
  hasAttachment: z.boolean(),
  preview: z.string(),
});

export const threadSchema = z.object({ id: idSchema, emailIds: z.array(idSchema) });

// A method response: the name, the arguments and the call id (RFC 8620
// section 3.2).
const invocationSchema = z.tuple([z.string(), z.record(z.string(), z.unknown()), z.string()]);

export const responseSchema = z.object({
  methodResponses: z.array(invocationSchema),
  sessionState: z.string(),
});

export const methodErrorSchema = z.object({ type: z.string() });

// The answer of a /get (RFC 8620 section 5.1) over one object shape.
export function getAnswerSchema<Item extends z.ZodMiniType>(item: Item) {
  return z.object({ state: z.string(), list: z.array(item), notFound: z.array(z.string()) });
}

export const changesAnswerSchema = z.object({
  oldState: z.string(),
  newState: z.string(),
  hasMoreChanges: z.boolean(),
  created: z.array(idSchema),
  updated: z.array(idSchema),
  destroyed: z.array(idSchema),
});

// RFC 8620 section 5.5: total rides along when asked, limit when lowered.
export const queryAnswerSchema = z.object({
  queryState: z.string(),
  position: countSchema,
  ids: z.array(idSchema),
  total: z.optional(countSchema),
  limit: z.optional(z.int().check(z.positive())),
});

// Why one object of a /set was not written (RFC 8620 section 5.3).
const setErrorSchema = z.object({ type: z.string() });

// The answer of a /set that updates and destroys (RFC 8620 section 5.3);
// a server may leave an empty map out instead of naming it null.
export const setAnswerSchema = z.object({
  oldState: z.nullish(z.string()),
  newState: z.string(),
  updated: z.nullish(z.record(z.string(), z.unknown())),
  notUpdated: z.nullish(z.record(z.string(), setErrorSchema)),
  destroyed: z.nullish(z.array(idSchema)),
  notDestroyed: z.nullish(z.record(z.string(), setErrorSchema)),
});

// The problem details of a request that was not run (RFC 8620 section 3.6.1).
export const problemSchema = z.object({ type: z.string(), limit: z.optional(z.string()) });

export type SessionObject = z.infer<typeof sessionObjectSchema>;
export type Mailbox = z.infer<typeof mailboxSchema>;
export type EmailState = z.infer<typeof emailStateSchema>;
export type EmailHeader = z.infer<typeof emailHeaderSchema>;
export type Thread = z.infer<typeof threadSchema>;
export type Invocation = z.infer<typeof invocationSchema>;
export type ChangesAnswer = z.infer<typeof changesAnswerSchema>;
