// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "../schema";
import { countSchema, idSchema } from "./schemas";

const MIB = 1024 * 1024;

// What one body value may weigh when a message opens.
export const BODY_VALUE_BYTES = 4 * MIB;

// The cap of the second ask, for a value the first one cut short; it
// stays inside the proxy's answer cap of 16 MiB.
export const BODY_VALUE_BYTES_LARGE = 12 * MIB;

// The properties one opened message is asked for (RFC 8621 section 4.2).
// htmlBody is among them, which the proxy asks of every body request.
export const BODY_PROPERTIES = [
  "bodyStructure",
  "textBody",
  "htmlBody",
  "attachments",
  "bodyValues",
  "header:Authentication-Results:asRaw:all",
  "header:Content-Type:asRaw",
] as const;

// One part with the default body properties (RFC 8621 section 4.1.4).
// subParts is not asked for, so a part's children are left out.
export const emailBodyPartSchema = z.object({
  partId: z.string().nullable(),
  blobId: z.string().nullable(),
  size: countSchema,
  name: z.string().nullable(),
  type: z.string(),
  charset: z.string().nullable(),
  disposition: z.string().nullable(),
  cid: z.string().nullable(),
  language: z.array(z.string()).nullable(),
  location: z.string().nullable(),
});

export const emailBodyValueSchema = z.object({
  value: z.string(),
  isEncodingProblem: z.boolean(),
  isTruncated: z.boolean(),
});

// One email as a body request answers it.
export const bodyAnswerSchema = z.object({
  id: idSchema,
  bodyStructure: emailBodyPartSchema,
  textBody: z.array(emailBodyPartSchema),
  htmlBody: z.array(emailBodyPartSchema),
  attachments: z.array(emailBodyPartSchema),
  bodyValues: z.record(z.string(), emailBodyValueSchema),
  // Every instance in the order of the message, the topmost first.
  "header:Authentication-Results:asRaw:all": z.array(z.string()),
  "header:Content-Type:asRaw": z.string().nullable(),
});

export type EmailBodyPart = z.infer<typeof emailBodyPartSchema>;
export type EmailBodyValue = z.infer<typeof emailBodyValueSchema>;
export type BodyAnswer = z.infer<typeof bodyAnswerSchema>;
