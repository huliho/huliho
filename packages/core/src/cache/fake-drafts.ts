// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Email/set with creates over the fake's emails, as RFC 8621 section 4.6
// has it for a draft and the e2e mock answers it: the tree read into its
// text, its HTML and its attachments, every blob checked against the
// held emails and the uploads, the email filed in its one mailbox in the
// thread it answers or one of its own. The updates and destroys of the
// call run after the creates, so a create beside a destroy replaces.

import { writeFlowed } from "../compose/flowed-write";
import type { EmailBodyPart } from "../jmap/body";
import type { EmailHeader } from "../jmap/schemas";
import { z } from "../schema";
import type { FakeJmap } from "./fake-jmap";
import { IDENTITY } from "./fake-sends";
import { UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";
import { emailSet, recountMailboxes, setArgsOf } from "./fake-writes";
import type { FakeBody } from "./fake-writes";
import type { DraftAttachment, DraftContent } from "./store";

// A new rich draft to Mo with a subject and one line of text, from the
// fake's identity, with what a test sets over it.
export function draftContent(overrides: Partial<DraftContent> = {}): DraftContent {
  return {
    kind: "new",
    parentId: null,
    mode: "rich",
    document: { type: "doc" },
    html: "<p>Hi</p>",
    text: writeFlowed([{ depth: 0, text: "Hi" }]),
    to: [{ name: "Mo", email: "mo@example.test" }],
    cc: [],
    bcc: [],
    subject: "Lunch",
    identity: { id: IDENTITY.id, name: IDENTITY.name, email: IDENTITY.email },
    inReplyTo: [],
    references: [],
    attachments: [],
    ...overrides,
  };
}

// What the attached file of a test weighs.
const DRAFT_FILE_BYTES = 10;

// A small PDF as a draft attaches it, under the blob id the fake gives
// its first upload.
export const DRAFT_FILE: DraftAttachment = {
  blobId: "upload-1",
  name: "plan.pdf",
  type: "application/pdf",
  size: DRAFT_FILE_BYTES,
  cid: null,
  disposition: "attachment",
};

// The keywords a create may carry: the system flags a server stores.
const CREATE_KEYWORDS = new Set(["$draft", "$seen", "$flagged", "$answered"]);
// The body lists a client of this server never sends: it sends the tree.
const FLAT_BODY = ["textBody", "htmlBody", "attachments"];
// The longest preview RFC 8621 section 4.1.4 asks for.
const PREVIEW_CHARS = 256;

const addressesSchema = z.nullable(
  z.array(z.object({ name: z.nullable(z.string()), email: z.string() })),
);
const idsSchema = z.nullable(z.array(z.string()));
const createSetSchema = z.object({ accountId: z.string(), ifInState: z.nullish(z.string()) });

type SetError = Record<string, unknown>;

function isRecord(value: unknown): value is Args {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringAt(record: Args, key: string): string | undefined {
  const value = new Map(Object.entries(record)).get(key);
  return typeof value === "string" ? value : undefined;
}

function leavesOf(node: unknown): Args[] {
  if (!isRecord(node)) {
    return [];
  }
  const below = node["subParts"];
  return Array.isArray(below) ? below.flatMap(leavesOf) : [node];
}

function valuesOf(object: Args): Map<string, unknown> {
  return new Map(Object.entries(isRecord(object["bodyValues"]) ? object["bodyValues"] : {}));
}

// Whether a blob id names an upload or a part of an email the fake holds.
function resolves(server: FakeJmap, blobId: string): boolean {
  return (
    server.uploads.has(blobId) ||
    [...server.emails.keys()].some((id) => blobId === id || blobId.startsWith(`${id}-`))
  );
}

function mailboxOf(server: FakeJmap, object: Args): string | null {
  const named = isRecord(object["mailboxIds"]) ? Object.entries(object["mailboxIds"]) : [];
  const [first] = named;
  if (named.length !== 1 || first === undefined || first[1] !== true) {
    return null;
  }
  return server.mailboxes.has(first[0]) ? first[0] : null;
}

function keywordsHold(object: Args): boolean {
  const keywords = isRecord(object["keywords"]) ? Object.entries(object["keywords"]) : [];
  return keywords.every(([keyword, on]) => CREATE_KEYWORDS.has(keyword) && on === true);
}

// What the rows refuse in a create, before anything is written.
function createError(server: FakeJmap, object: Args): SetError | null {
  const flat = FLAT_BODY.filter((property) => property in object);
  if (flat.length > 0 || !isRecord(object["bodyStructure"])) {
    return { type: "invalidProperties", properties: flat.length > 0 ? flat : ["bodyStructure"] };
  }
  if (mailboxOf(server, object) === null) {
    return { type: "invalidProperties", properties: ["mailboxIds"] };
  }
  if (!keywordsHold(object)) {
    return { type: "invalidProperties", properties: ["keywords"] };
  }
  const values = valuesOf(object);
  const leaves = leavesOf(object["bodyStructure"]);
  const partIds = leaves.flatMap((leaf) => stringAt(leaf, "partId") ?? []);
  if (partIds.some((partId) => !isRecord(values.get(partId)))) {
    return { type: "invalidProperties", properties: ["bodyValues"] };
  }
  const blobIds = leaves.flatMap((leaf) => stringAt(leaf, "blobId") ?? []);
  const notFound = blobIds.filter((blobId) => !resolves(server, blobId));
  return notFound.length > 0 ? { type: "blobNotFound", notFound } : null;
}

// The media type of a leaf, given as `type` or in the raw header.
function typeOf(leaf: Args): string | undefined {
  return stringAt(leaf, "type") ?? stringAt(leaf, "header:Content-Type")?.split(";")[0]?.trim();
}

function valueOf(object: Args, type: string): string | undefined {
  const leaf = leavesOf(object["bodyStructure"]).find((one) => typeOf(one) === type);
  const partId = leaf === undefined ? undefined : stringAt(leaf, "partId");
  const value = partId === undefined ? undefined : valuesOf(object).get(partId);
  return isRecord(value) ? stringAt(value, "value") : undefined;
}

// The thread of the held email the draft answers, else one of its own.
function threadOf(server: FakeJmap, inReplyTo: readonly string[], id: string): string {
  const parent = [...server.emails.values()].find((row) =>
    (row.messageId ?? []).some((messageId) => inReplyTo.includes(messageId)),
  );
  return parent?.threadId ?? `t-${id}`;
}

// The blob of the new email's part at a leaf's place, counted from one in
// the order of the leaves, as the bridge names a section.
function sectionBlob(id: string, index: number): string {
  return `${id}-${String(index + 1)}`;
}

function blobPart(server: FakeJmap, leaf: Args, blobId: string): EmailBodyPart {
  const given = stringAt(leaf, "blobId") ?? "";
  return {
    partId: null,
    blobId,
    size: server.uploads.get(given)?.size ?? 0,
    name: stringAt(leaf, "name") ?? null,
    type: typeOf(leaf) ?? "application/octet-stream",
    charset: null,
    disposition: stringAt(leaf, "disposition") ?? "attachment",
    cid: stringAt(leaf, "cid") ?? null,
    language: null,
    location: null,
  };
}

function bodyOf(server: FakeJmap, object: Args, id: string): FakeBody {
  const text = valueOf(object, "text/plain");
  const html = valueOf(object, "text/html");
  const attachments = leavesOf(object["bodyStructure"]).flatMap((leaf, index) =>
    stringAt(leaf, "blobId") === undefined ? [] : [blobPart(server, leaf, sectionBlob(id, index))],
  );
  return {
    ...(text === undefined ? {} : { text }),
    ...(html === undefined ? {} : { html }),
    attachments,
  };
}

// The tree a create filed, each leaf under the blob of the new email's
// part, for an Email/get that asks the bodyStructure.
function storedTree(node: Args, id: string, count: { leaves: number }): Args {
  const below = node["subParts"];
  if (Array.isArray(below)) {
    const parts = below.filter((part) => isRecord(part));
    return {
      blobId: null,
      type: typeOf(node) ?? "multipart/mixed",
      subParts: parts.map((part) => storedTree(part, id, count)),
    };
  }
  const blobId = sectionBlob(id, count.leaves);
  count.leaves += 1;
  return { blobId, type: typeOf(node) ?? "text/plain", subParts: null };
}

// A property as the schema reads it, null where it holds anything else.
function read<Value>(schema: z.ZodMiniType<Value>, value: unknown): Value | null {
  const parsed = schema.safeParse(value);
  return parsed.success ? parsed.data : null;
}

function headerOf(server: FakeJmap, object: Args, id: string): EmailHeader {
  const given = new Map(Object.entries(object));
  const field = (name: string): EmailHeader["to"] => read(addressesSchema, given.get(name) ?? null);
  const inReplyTo = read(idsSchema, object["inReplyTo"] ?? null);
  const keywords = isRecord(object["keywords"]) ? Object.keys(object["keywords"]) : [];
  return {
    id,
    blobId: id,
    threadId: threadOf(server, inReplyTo ?? [], id),
    mailboxIds: { [mailboxOf(server, object) ?? ""]: true },
    keywords: Object.fromEntries(keywords.map((keyword) => [keyword, true])),
    size: JSON.stringify(object).length,
    receivedAt: new Date().toISOString().replace(/\.\d+/u, ""),
    messageId: [`${id}@example.test`],
    inReplyTo,
    references: read(idsSchema, object["references"] ?? null),
    sender: null,
    from: field("from"),
    to: field("to"),
    cc: field("cc"),
    bcc: field("bcc"),
    replyTo: field("replyTo"),
    subject: stringAt(object, "subject") ?? null,
    sentAt: null,
    hasAttachment: false,
    preview: "",
  };
}

// The email a create files, its body kept for the reads after it.
function filed(server: FakeJmap, object: Args): Args {
  server.nextEmail += 1;
  const id = `draft-${String(server.nextEmail)}`;
  const body = bodyOf(server, object, id);
  const header = headerOf(server, object, id);
  const row = {
    ...header,
    hasAttachment: (body.attachments ?? []).length > 0,
    preview: (body.text ?? "").slice(0, PREVIEW_CHARS),
  };
  server.bodies.set(id, body);
  if (isRecord(object["bodyStructure"])) {
    server.structures.set(id, storedTree(object["bodyStructure"], id, { leaves: 0 }));
  }
  server.addEmail(row);
  recountMailboxes(server, Object.keys(row.mailboxIds));
  return { id, blobId: id, threadId: row.threadId, size: row.size };
}

// Email/set with its creates first: each refused one answers its error,
// each other lands and its creation id names it for the calls after it
// (RFC 8620 section 5.3); then the updates and destroys.
export function emailSetWithCreates(server: FakeJmap, raw: Args, made: Map<string, string>): Args {
  setArgsOf(server, createSetSchema, raw);
  const creates = isRecord(raw["create"]) ? Object.entries(raw["create"]) : [];
  const oldState = String(server.sequence);
  const created: [string, Args][] = [];
  const notCreated: [string, SetError][] = [];
  for (const [key, object] of creates) {
    const refusal = isRecord(object)
      ? (server.createRefusal ?? createError(server, object))
      : { type: "invalidProperties" };
    if (refusal !== null || !isRecord(object)) {
      notCreated.push([key, refusal ?? { type: "invalidProperties" }]);
      continue;
    }
    const answer = filed(server, object);
    made.set(key, String(answer["id"]));
    created.push([key, answer]);
  }
  const rest = Object.fromEntries(Object.entries(raw).filter(([name]) => name !== "create"));
  return {
    ...emailSet(server, rest),
    accountId: UPSTREAM,
    oldState,
    created: created.length > 0 ? Object.fromEntries(created) : null,
    notCreated: notCreated.length > 0 ? Object.fromEntries(notCreated) : null,
  };
}
