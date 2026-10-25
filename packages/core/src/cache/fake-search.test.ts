// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import type { EmailHeader } from "../jmap/schemas";
import { CORE_CAPABILITY, MAIL_CAPABILITY } from "../jmap/schemas";
import { z } from "../schema";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, mailbox } from "./fake-jmap";

const MAILBOXES = ["inbox", "archive", "junk", "trash"];

const idsSchema = z.object({ ids: z.array(z.string()), total: z.optional(z.int()) });

function serve(rows: readonly EmailHeader[]): { server: FakeJmap; client: JmapClient } {
  const server = new FakeJmap();
  for (const id of MAILBOXES) {
    server.putMailbox(mailbox(id, id));
  }
  for (const one of rows) {
    server.addEmail(one);
  }
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT) };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

// One Email/query newest first; the answer's name and arguments.
async function query(
  client: JmapClient,
  filter: unknown,
  more: Record<string, unknown> = {},
): Promise<[string, Record<string, unknown>]> {
  const sort = [{ property: "receivedAt", isAscending: false }];
  const args = { accountId: UPSTREAM, filter, sort, ...more };
  const [answer] = await client.request([{ name: "Email/query", arguments: args, id: "q" }]);
  if (answer === undefined) {
    throw new Error("no answer");
  }
  return [answer[0], answer[1]];
}

async function idsOf(client: JmapClient, filter: unknown): Promise<string[]> {
  const [name, args] = await query(client, filter);
  expect(name).toBe("Email/query");
  return idsSchema.parse(args).ids;
}

// An email whose search fields differ from the fixture's.
function row(id: string, seconds: number, facts: Partial<EmailHeader> = {}): EmailHeader {
  return { ...email(id, { receivedAt: at(seconds) }), ...facts };
}

const BLOM = [{ name: "Pieter Blom", email: "pieter@blom.example" }];

test("a text matches every term in any case over the subject, the sender and the preview", async () => {
  const { client } = serve([
    row("e1", 1, { subject: "Offerte badkamer" }),
    row("e2", 2, { from: BLOM }),
    row("e3", 3, { preview: "Badkamer en tegels" }),
    row("e4", 4),
  ]);
  expect(await idsOf(client, { text: "BADKAMER" })).toEqual(["e3", "e1"]);
  expect(await idsOf(client, { text: "offerte badkamer" })).toEqual(["e1"]);
  expect(await idsOf(client, { text: '"badkamer en"' })).toEqual(["e3"]);
  expect(await idsOf(client, { text: "blom" })).toEqual(["e2"]);
});

test("a sender matches the name or the address and nothing else", async () => {
  const { client } = serve([row("e1", 1, { subject: "Blom" }), row("e2", 2, { from: BLOM })]);
  expect(await idsOf(client, { from: "pieter blom" })).toEqual(["e2"]);
  expect(await idsOf(client, { from: "pieter@blom" })).toEqual(["e2"]);
  expect(await idsOf(client, { from: "offerte" })).toEqual([]);
});

test("after holds the instant itself and before leaves it out (RFC 8621 section 4.4.1)", async () => {
  const { client } = serve([1, 2, 3, 4, 5].map((seconds) => row(`e${String(seconds)}`, seconds)));
  expect(await idsOf(client, { after: at(2), before: at(4) })).toEqual(["e3", "e2"]);
  const [name, args] = await query(client, { after: "2026-01-01T00:00:02+00:00" });
  expect([name, args]).toEqual(["error", { type: "invalidArguments" }]);
});

test("hasAttachment keeps the emails whose flag matches", async () => {
  const { client } = serve([row("e1", 1, { hasAttachment: true }), row("e2", 2)]);
  expect(await idsOf(client, { hasAttachment: true })).toEqual(["e1"]);
  expect(await idsOf(client, { hasAttachment: false })).toEqual(["e2"]);
});

test("inMailboxOtherThan keeps an email that stands in a mailbox outside the list", async () => {
  const { client } = serve([
    email("e1", { receivedAt: at(1), mailboxIds: ["inbox"] }),
    email("e2", { receivedAt: at(2), mailboxIds: ["junk"] }),
    email("e3", { receivedAt: at(3), mailboxIds: ["trash"] }),
    email("e4", { receivedAt: at(4), mailboxIds: ["junk", "archive"] }),
  ]);
  expect(await idsOf(client, { inMailboxOtherThan: ["junk", "trash"] })).toEqual(["e4", "e1"]);
});

test("no filter and a null filter answer every email, one per thread on request", async () => {
  const { client } = serve([
    email("e1", { receivedAt: at(1), threadId: "t1", mailboxIds: ["inbox"] }),
    email("e2", { receivedAt: at(2), threadId: "t1", mailboxIds: ["junk"] }),
    email("e3", { receivedAt: at(3), threadId: "t2", mailboxIds: ["trash"] }),
  ]);
  expect(await idsOf(client, null)).toEqual(["e3", "e2", "e1"]);
  const [, args] = await query(client, undefined, { collapseThreads: true, calculateTotal: true });
  expect(idsSchema.parse(args)).toEqual({ ids: ["e3", "e2"], total: 2 });
});

test("an operator or a property no condition names answers unsupportedFilter", async () => {
  const { client } = serve([row("e1", 1)]);
  const operator = { operator: "AND", conditions: [{ text: "x" }] };
  expect(await query(client, operator)).toEqual(["error", { type: "unsupportedFilter" }]);
  expect(await query(client, { subject: "x" })).toEqual(["error", { type: "unsupportedFilter" }]);
});

test("the scripted failure answers a search and leaves a mailbox window alone", async () => {
  const { server, client } = serve([row("e1", 1)]);
  server.searchError = "serverUnavailable";
  expect(await query(client, { text: "x" })).toEqual(["error", { type: "serverUnavailable" }]);
  expect(await idsOf(client, { inMailbox: "inbox" })).toEqual(["e1"]);
});

test("a held search waits for its release while a mailbox window answers", async () => {
  const { server, client } = serve([row("e1", 1, { subject: "Offerte" })]);
  server.holdSearches = true;
  const search = idsOf(client, { text: "offerte" });
  await vi.waitFor(() => {
    expect(server.waiting).toBe(1);
  });
  expect(await idsOf(client, { inMailbox: "inbox" })).toEqual(["e1"]);
  server.releaseSearches();
  expect(await search).toEqual(["e1"]);
  expect(server.waiting).toBe(0);
});

test("an abort rejects a held search the way fetch does and leaves nothing waiting", async () => {
  const { server } = serve([row("e1", 1)]);
  server.holdSearches = true;
  const controller = new AbortController();
  const body = {
    using: [CORE_CAPABILITY, MAIL_CAPABILITY],
    methodCalls: [["Email/query", { accountId: UPSTREAM, filter: { text: "x" } }, "q"]],
  };
  const sent = server.fetch(`/api/jmap/${ACCOUNT}`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-requested-with": "huliho" },
    body: JSON.stringify(body),
    signal: controller.signal,
  });
  expect(server.waiting).toBe(1);
  controller.abort();
  await expect(sent).rejects.toMatchObject({ name: "AbortError" });
  expect(server.waiting).toBe(0);
  server.releaseSearches();
  expect(server.waiting).toBe(0);
});

const WORDS = ["offerte", "factuur", "badkamer", "tegels"];
const SENDERS = [
  { name: "Pieter Blom", email: "pieter@blom.example" },
  { name: "Iris Bakker", email: "iris@bakker.example" },
  { name: "Jonas Verhulst", email: "jonas@verhulst.example" },
];

// The latest second of the fixture's day a generated email or date falls on.
const LAST_SECOND = 59;
// The most emails one generated corpus holds.
const MOST_EMAILS = 12;

const rowArbitrary = fc.record({
  word: fc.constantFrom(...WORDS),
  sender: fc.constantFrom(...SENDERS),
  seconds: fc.integer({ min: 0, max: LAST_SECOND }),
  attachment: fc.boolean(),
  boxes: fc.subarray(MAILBOXES, { minLength: 1 }),
});

type Generated = typeof rowArbitrary extends fc.Arbitrary<infer Row> ? Row : never;

function generated(rows: readonly Generated[]): EmailHeader[] {
  return rows.map((one, index) => ({
    ...email(`e${String(index)}`, { receivedAt: at(one.seconds), mailboxIds: one.boxes }),
    subject: `Over ${one.word}`,
    from: [one.sender],
    hasAttachment: one.attachment,
  }));
}

// The emails every condition keeps when each is asked alone.
async function intersected(
  client: JmapClient,
  condition: Record<string, unknown>,
): Promise<Set<string>> {
  const every = await idsOf(client, null);
  const answers = await Promise.all(
    Object.entries(condition).map(async ([key, value]) => idsOf(client, { [key]: value })),
  );
  return new Set(every.filter((id) => answers.every((alone) => alone.includes(id))));
}

const conditionArbitrary = fc.record(
  {
    text: fc.constantFrom(...WORDS),
    from: fc.constantFrom("blom", "bakker", "jonas"),
    after: fc.integer({ min: 0, max: LAST_SECOND }).map(at),
    before: fc.integer({ min: 0, max: LAST_SECOND }).map(at),
    hasAttachment: fc.boolean(),
    inMailboxOtherThan: fc.subarray(MAILBOXES),
  },
  { requiredKeys: [] },
);

test("the conditions of one filter combine: its answer is each condition's answer intersected", async () => {
  await fc.assert(
    fc.asyncProperty(
      fc.array(rowArbitrary, { maxLength: MOST_EMAILS }),
      conditionArbitrary,
      async (rows, condition) => {
        const { client } = serve(generated(rows));
        const all = new Set(await idsOf(client, condition));
        expect(all).toEqual(await intersected(client, condition));
      },
    ),
  );
});
