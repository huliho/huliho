// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailBodyPart } from "../jmap/body";
import type { EmailHeader, Mailbox } from "../jmap/schemas";
import { z } from "../schema";
import type { FakeJmap } from "./fake-jmap";
import { MethodError, UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";

const SEEN = "$seen";

// What a test gives an email for a body; an email without one answers
// its preview as plain text.
export interface FakeBody {
  text?: string;
  html?: string;
  attachments?: EmailBodyPart[];
  // Every Authentication-Results header, the topmost first.
  authenticationResults?: string[];
  contentType?: string;
}

const bodyArgsSchema = z.object({
  fetchTextBodyValues: z.boolean().optional(),
  fetchHTMLBodyValues: z.boolean().optional(),
  maxBodyValueBytes: z.number().int().nonnegative().optional(),
});

interface Leaf {
  partId: string;
  part: EmailBodyPart;
  value: string;
}

// The body lists of one email: the text part and the HTML part, each
// standing in for the other where one is missing (RFC 8621 section
// 4.1.4).
interface Alternatives {
  root: EmailBodyPart;
  text: Leaf[];
  html: Leaf[];
}

function part(emailId: string, partId: string | null, type: string, size: number): EmailBodyPart {
  return {
    partId,
    blobId: partId === null ? null : `${emailId}-${partId}`,
    size,
    name: null,
    type,
    charset: partId === null ? null : "utf-8",
    disposition: null,
    cid: null,
    language: null,
    location: null,
  };
}

function leaf(emailId: string, partId: string, type: string, value: string | undefined): Leaf[] {
  const one = part(emailId, partId, type, value?.length ?? 0);
  return value === undefined ? [] : [{ partId, part: one, value }];
}

function alternativesOf(emailId: string, body: FakeBody): Alternatives {
  const text = leaf(emailId, "1", "text/plain", body.text);
  const html = leaf(emailId, "2", "text/html", body.html);
  const [first, second] = [...text, ...html];
  if (first === undefined) {
    return { root: part(emailId, null, "multipart/mixed", 0), text, html };
  }
  if (second === undefined) {
    return { root: first.part, text: [first], html: [first] };
  }
  return { root: part(emailId, null, "multipart/alternative", 0), text, html };
}

// The values of the parts asked for, each cut at the cap (RFC 8621
// section 4.2).
function valuesOf(leaves: readonly Leaf[], cap: number): Args {
  return Object.fromEntries(
    leaves.map(({ partId, value }) => [
      partId,
      { value: value.slice(0, cap), isEncodingProblem: false, isTruncated: value.length > cap },
    ]),
  );
}

// The body properties of one email, its values under the smaller of the
// request's cap and the server's own.
export function bodyOf(server: FakeJmap, row: EmailHeader, raw: Args): Args {
  const args = bodyArgsSchema.parse(raw);
  const held = server.bodies.get(row.id) ?? { text: row.preview };
  const { root, text, html } = alternativesOf(row.id, held);
  const asked = [
    ...(args.fetchTextBodyValues === true ? text : []),
    ...(args.fetchHTMLBodyValues === true ? html : []),
  ];
  const cap = Math.min(args.maxBodyValueBytes ?? server.bodyValueCap, server.bodyValueCap);
  return {
    bodyStructure: root,
    textBody: text.map((one) => one.part),
    htmlBody: html.map((one) => one.part),
    attachments: held.attachments ?? [],
    bodyValues: valuesOf(asked, cap),
    "header:Authentication-Results:asRaw:all": held.authenticationResults ?? [],
    "header:Content-Type:asRaw": held.contentType ?? ` ${root.type}; charset=utf-8`,
  };
}

const bodyAskSchema = z.object({
  properties: z.array(z.string()).nullish(),
  fetchTextBodyValues: z.boolean().optional(),
  fetchHTMLBodyValues: z.boolean().optional(),
  fetchAllBodyValues: z.boolean().optional(),
});

const requestSchema = z.object({
  methodCalls: z.array(z.tuple([z.string(), bodyAskSchema, z.string()])),
});

// Whether the proxy's sanitizer can find the HTML parts of the answer:
// never when every value is asked, else when htmlBody is asked with
// the values.
function isCovered(ask: z.infer<typeof bodyAskSchema>): boolean {
  if (ask.fetchAllBodyValues === true) {
    return false;
  }
  const properties = ask.properties ?? null;
  const asksValues =
    ask.fetchTextBodyValues === true ||
    ask.fetchHTMLBodyValues === true ||
    properties?.includes("bodyValues") === true;
  return !asksValues || properties === null || properties.includes("htmlBody");
}

// Whether the proxy lets a request through: it refuses a body request
// that asks every value or leaves htmlBody out, since its sanitizer
// could not find the HTML parts of the answer.
export function passesProxy(body: unknown): boolean {
  const request = requestSchema.safeParse(body);
  if (!request.success) {
    return true;
  }
  return request.data.methodCalls.every(([name, args]) => name !== "Email/get" || isCovered(args));
}

function isUnread(row: EmailHeader, mailboxId: string): boolean {
  return mailboxId in row.mailboxIds && !(SEEN in row.keywords);
}

// The four counts of a mailbox from the emails the server holds.
function counted(server: FakeJmap, row: Mailbox): Mailbox {
  const inside = [...server.emails.values()].filter((email) => row.id in email.mailboxIds);
  const unread = inside.filter((email) => isUnread(email, row.id));
  return {
    ...row,
    totalEmails: inside.length,
    unreadEmails: unread.length,
    totalThreads: new Set(inside.map((email) => email.threadId)).size,
    unreadThreads: new Set(unread.map((email) => email.threadId)).size,
  };
}

// Brings the counts of the named mailboxes in line with the emails; a
// mailbox whose counts move is logged as updated.
export function recountMailboxes(server: FakeJmap, mailboxIds: readonly string[]): void {
  for (const id of mailboxIds) {
    const row = server.mailboxes.get(id);
    const fresh = row === undefined ? undefined : counted(server, row);
    if (fresh !== undefined && JSON.stringify(fresh) !== JSON.stringify(row)) {
      server.putMailbox(fresh);
    }
  }
}

const setArgsSchema = z.object({
  accountId: z.string(),
  ifInState: z.string().nullish(),
  update: z.record(z.string(), z.record(z.string(), z.unknown())).nullish(),
  destroy: z.array(z.string()).nullish(),
});

const SETS = ["keywords", "mailboxIds"] as const;

type State = Pick<EmailHeader, "keywords" | "mailboxIds">;

// The set and the member a path names (RFC 6901 section 4); null for a
// path off both sets or a mailbox the server does not hold.
function targetOf(
  server: FakeJmap,
  path: string,
): { set: (typeof SETS)[number]; key: string } | null {
  const set = SETS.find((name) => path.startsWith(`${name}/`));
  if (set === undefined) {
    return null;
  }
  const key = path
    .slice(set.length + 1)
    .replaceAll("~1", "/")
    .replaceAll("~0", "~");
  return key === "" || (set === "mailboxIds" && !server.mailboxes.has(key)) ? null : { set, key };
}

// The keywords and mailboxes after a patch of their paths, each true or
// null (RFC 8620 section 5.3); null for a patch that names anything
// else.
function stateAfter(server: FakeJmap, row: EmailHeader, patch: Args): State | null {
  const held = {
    keywords: new Map(Object.entries(row.keywords)),
    mailboxIds: new Map(Object.entries(row.mailboxIds)),
  };
  for (const [path, value] of Object.entries(patch)) {
    const target = targetOf(server, path);
    if (target === null || (value !== true && value !== null)) {
      return null;
    }
    if (value === null) {
      held[target.set].delete(target.key);
    } else {
      held[target.set].set(target.key, true);
    }
  }
  return {
    keywords: Object.fromEntries(held.keywords),
    mailboxIds: Object.fromEntries(held.mailboxIds),
  };
}

// One update: the SetError type when it is refused, null once written.
// An email always stands in one mailbox at least (RFC 8621 section
// 4.1.1).
function update(server: FakeJmap, id: string, patch: Args): string | null {
  const refusal = server.refused.get(id);
  const row = server.emails.get(id);
  if (refusal !== undefined || row === undefined) {
    return refusal ?? "notFound";
  }
  const after = stateAfter(server, row, patch);
  if (after === null || Object.keys(after.mailboxIds).length === 0) {
    return "invalidProperties";
  }
  server.amend(id, after);
  recountMailboxes(server, [...Object.keys(row.mailboxIds), ...Object.keys(after.mailboxIds)]);
  return null;
}

// One destroy: the SetError type when it is refused, null once done.
function destroy(server: FakeJmap, id: string): string | null {
  const refusal = server.refused.get(id);
  const row = server.emails.get(id);
  if (refusal !== undefined || row === undefined) {
    return refusal ?? "notFound";
  }
  server.destroyEmail(id);
  recountMailboxes(server, Object.keys(row.mailboxIds));
  return null;
}

function named<Value>(entries: [string, Value][]): Record<string, Value> | null {
  return entries.length === 0 ? null : Object.fromEntries(entries);
}

function refusals(
  outcomes: readonly [string, string | null][],
): Record<string, { type: string }> | null {
  return named(
    outcomes.flatMap(([id, refusal]): [string, { type: string }][] =>
      refusal === null ? [] : [[id, { type: refusal }]],
    ),
  );
}

// The arguments of a set the account takes, in the state it names.
function setArgsOf(server: FakeJmap, raw: Args): z.infer<typeof setArgsSchema> {
  const args = setArgsSchema.safeParse(raw);
  if (!args.success) {
    throw new MethodError("invalidArguments");
  }
  if (args.data.accountId !== UPSTREAM) {
    throw new MethodError("accountNotFound");
  }
  if (server.readOnly) {
    throw new MethodError("accountReadOnly");
  }
  const { ifInState } = args.data;
  if (ifInState !== null && ifInState !== undefined && ifInState !== String(server.sequence)) {
    throw new MethodError("stateMismatch");
  }
  return args.data;
}

// Email/set for updates of keywords and mailboxes and for destroys, as
// RFC 8621 section 4.6 has it.
export function emailSet(server: FakeJmap, raw: Args): Args {
  const args = setArgsOf(server, raw);
  const oldState = String(server.sequence);
  const updates = Object.entries(args.update ?? {});
  const destroys = args.destroy ?? [];
  if (updates.length + destroys.length > server.maxObjectsInSet) {
    throw new MethodError("requestTooLarge");
  }
  const updated = updates.map(([id, patch]): [string, string | null] => [
    id,
    update(server, id, patch),
  ]);
  const destroyed = destroys.map((id): [string, string | null] => [id, destroy(server, id)]);
  const done = destroyed.flatMap(([id, refusal]) => (refusal === null ? [id] : []));
  return {
    accountId: UPSTREAM,
    oldState,
    newState: String(server.sequence),
    created: null,
    updated: named(updated.flatMap(([id, refusal]) => (refusal === null ? [[id, null]] : []))),
    destroyed: done.length === 0 ? null : done,
    notCreated: null,
    notUpdated: refusals(updated),
    notDestroyed: refusals(destroyed),
  };
}
