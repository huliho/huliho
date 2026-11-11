// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Email/set with creates, as RFC 8621 section 4.6 has it for a draft:
// the bodyStructure tree read into its text, its HTML and its
// attachments, every blobId checked against the corpus and the uploads,
// the email filed into its one mailbox in a thread of its own or in the
// thread it answers. The updates and destroys of the call run after the
// creates, so a create beside a destroy replaces a draft.

import type { MailServer } from "./mail-answers";
import { part } from "./mail-bodies";
import { UPSTREAM } from "./mail-corpus";
import type { CorpusEmail } from "./mail-corpus";
import { isRecord } from "./mail-folder-rules";
import { emailSet, fileEmail } from "./mail-writes";

type Args = Record<string, unknown>;
type SetError = Record<string, unknown>;

// The keywords a create may carry: the ones a draft has and the system
// flags a server stores.
const CREATE_KEYWORDS = new Set(["$draft", "$seen", "$flagged", "$answered"]);
// The body properties a client of this server never sends: it always
// sends the tree.
const FLAT_BODY = ["textBody", "htmlBody", "attachments"];
// The header properties a saved draft answers as they were given.
const SAVED_FIELDS = ["from", "to", "cc", "bcc", "replyTo", "subject", "inReplyTo", "references"];
// Where a corpus message's id ends in its Message-ID.
const MESSAGE_ID_DOMAIN = "@example.test";
// The longest preview RFC 8621 section 4.1.4 asks for.
const PREVIEW_CHARS = 256;

function stringAt(record: Args, key: string): string | undefined {
  const value = new Map(Object.entries(record)).get(key);
  return typeof value === "string" ? value : undefined;
}

// Every leaf of the tree, in order.
function leavesOf(node: unknown): Args[] {
  if (!isRecord(node)) {
    return [];
  }
  const below = node["subParts"];
  return Array.isArray(below) ? below.flatMap(leavesOf) : [node];
}

// The body values of a create by part id.
function valuesOf(object: Args): Map<string, unknown> {
  return new Map(Object.entries(isRecord(object["bodyValues"]) ? object["bodyValues"] : {}));
}

// Whether a blob id names an upload or a part of an email the corpus holds.
function resolves(server: MailServer, blobId: string): boolean {
  if (server.uploads.has(blobId)) {
    return true;
  }
  return [...server.corpus.emails.keys()].some(
    (id) => blobId === id || blobId.startsWith(`${id}-`),
  );
}

// The one mailbox a draft goes into, or null.
function mailboxOf(server: MailServer, object: Args): string | null {
  const named = isRecord(object["mailboxIds"]) ? Object.entries(object["mailboxIds"]) : [];
  const [first] = named;
  if (named.length !== 1 || first === undefined || first[1] !== true) {
    return null;
  }
  return server.mailboxes.some((row) => row["id"] === first[0]) ? first[0] : null;
}

function keywordsHold(object: Args): boolean {
  const keywords = isRecord(object["keywords"]) ? Object.entries(object["keywords"]) : [];
  return keywords.every(([keyword, on]) => CREATE_KEYWORDS.has(keyword) && on === true);
}

// What the rows refuse in a create, before anything is written.
function createError(server: MailServer, object: Args): SetError | null {
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

// The value of the first leaf of that type, from the body values.
function valueOf(object: Args, type: string): string | undefined {
  const leaf = leavesOf(object["bodyStructure"]).find((one) => one["type"] === type);
  const partId = leaf === undefined ? undefined : stringAt(leaf, "partId");
  const value = partId === undefined ? undefined : valuesOf(object).get(partId);
  return isRecord(value) ? stringAt(value, "value") : undefined;
}

// The thread of the corpus message the draft answers, else its own.
function threadOf(server: MailServer, object: Args, id: string): string {
  const answered = Array.isArray(object["inReplyTo"]) ? object["inReplyTo"] : [];
  const parent = answered
    .map((messageId) => String(messageId).replace(MESSAGE_ID_DOMAIN, ""))
    .map((parentId) => server.corpus.emails.get(parentId))
    .find((message) => message !== undefined);
  return parent?.threadId ?? `${id}-t`;
}

function senderOf(object: Args): { name: string; email: string } {
  const from: unknown = object["from"];
  const first: unknown = Array.isArray(from) ? from.at(0) : undefined;
  const email = isRecord(first) ? stringAt(first, "email") : undefined;
  const name = isRecord(first) ? stringAt(first, "name") : undefined;
  return { name: name ?? "", email: email ?? "" };
}

// The attachments of the tree, each with the facts it was given.
function attachmentsOf(server: MailServer, object: Args) {
  return leavesOf(object["bodyStructure"]).flatMap((leaf) => {
    const blobId = stringAt(leaf, "blobId");
    if (blobId === undefined) {
      return [];
    }
    return [
      part(stringAt(leaf, "type") ?? "application/octet-stream", {
        blobId,
        name: stringAt(leaf, "name") ?? null,
        disposition: stringAt(leaf, "disposition") ?? "attachment",
        cid: stringAt(leaf, "cid") ?? null,
        size: server.uploads.get(blobId)?.size ?? 0,
      }),
    ];
  });
}

// The email a create files, its body kept for the reads after it.
function filedDraft(server: MailServer, object: Args): Args {
  server.nextEmail += 1;
  const id = `draft-${String(server.nextEmail)}`;
  const keywords = isRecord(object["keywords"]) ? object["keywords"] : {};
  const text = valueOf(object, "text/plain");
  const html = valueOf(object, "text/html");
  const attachments = attachmentsOf(server, object);
  const message: CorpusEmail = {
    id,
    threadId: threadOf(server, object, id),
    mailboxId: mailboxOf(server, object) ?? "",
    receivedAt: new Date().toISOString(),
    seen: keywords["$seen"] === true,
    flagged: keywords["$flagged"] === true,
    draft: keywords["$draft"] === true,
    attachment: attachments.length > 0,
    from: senderOf(object),
    subject: stringAt(object, "subject") ?? "",
    preview: (text ?? "").slice(0, PREVIEW_CHARS),
  };
  server.corpus.bodies.set(id, {
    ...(text === undefined ? {} : { text }),
    ...(html === undefined ? {} : { html }),
    attachments,
  });
  const given = new Map(Object.entries(object));
  server.saved.set(
    id,
    Object.fromEntries(SAVED_FIELDS.map((field) => [field, given.get(field) ?? null])),
  );
  fileEmail(server, message);
  const size = JSON.stringify(object).length;
  return { id, blobId: id, threadId: message.threadId, size };
}

// The header fields a saved draft was given, over what the corpus shape
// would answer for it.
export function savedFields(server: MailServer, id: string): Args {
  return server.saved.get(id) ?? {};
}

// Email/set with its creates first: each refused one answers its error,
// each other lands and its creation id names it for the calls after it
// (RFC 8620 section 5.3); then the updates and destroys.
export function emailSetWithCreates(
  server: MailServer,
  args: Args,
  made: Map<string, string>,
): Args {
  const oldState = String(server.changes.length);
  const creates = isRecord(args["create"]) ? Object.entries(args["create"]) : [];
  const created: [string, Args][] = [];
  const notCreated: [string, SetError][] = [];
  for (const [key, object] of creates) {
    const refusal = isRecord(object) ? createError(server, object) : { type: "invalidProperties" };
    if (refusal !== null || !isRecord(object)) {
      notCreated.push([key, refusal ?? { type: "invalidProperties" }]);
      continue;
    }
    const answer = filedDraft(server, object);
    made.set(key, String(answer["id"]));
    created.push([key, answer]);
  }
  const rest = emailSet(server, args);
  return {
    ...rest,
    accountId: UPSTREAM,
    oldState,
    created: created.length > 0 ? Object.fromEntries(created) : null,
    notCreated: notCreated.length > 0 ? Object.fromEntries(notCreated) : null,
  };
}
