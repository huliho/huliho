// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { ACCOUNT, FakeJmap, at, email, mailbox } from "./fake-jmap";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import type { Mutation } from "./pending";
import { mayPatch } from "./rights";
import { queryWindow } from "./window";

const READ: Mutation = { type: "Email", id: "e1", patch: { "keywords/$seen": true } };
const FLAG: Mutation = { type: "Email", id: "e1", patch: { "keywords/$flagged": true } };

function withRights(id: string, rights: Partial<Mailbox["myRights"]>): Mailbox {
  const row = mailbox(id, id);
  return { ...row, myRights: { ...row.myRights, ...rights } };
}

// One email in the given mailboxes; the tree and the email are held.
async function serve(
  boxes: readonly Mailbox[],
  knobs: (server: FakeJmap) => void = () => undefined,
): Promise<{ client: JmapClient; store: MemoryMailStore }> {
  const server = new FakeJmap();
  for (const box of boxes) {
    server.putMailbox(box);
  }
  server.addEmail(email("e1", { mailboxIds: boxes.map((box) => box.id), receivedAt: at(1) }));
  knobs(server);
  vi.stubGlobal("fetch", server.fetch);
  const rig = { client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, boxes[0]?.id ?? "inbox", 0);
  return rig;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("an account that takes writes takes a keyword change its mailboxes allow", async () => {
  const { client, store } = await serve([mailbox("inbox", "inbox")]);
  expect(await mayPatch(client, store, READ)).toBe(true);
  expect(await mayPatch(client, store, FLAG)).toBe(true);
});

test("a read-only account takes no change", async () => {
  const { client, store } = await serve([mailbox("inbox", "inbox")], (server) => {
    server.readOnly = true;
  });
  expect(await mayPatch(client, store, READ)).toBe(false);
});

test("an account that takes no object per set takes no change", async () => {
  const { client, store } = await serve([mailbox("inbox", "inbox")], (server) => {
    server.maxObjectsInSet = 0;
  });
  expect(await mayPatch(client, store, READ)).toBe(false);
});

test("$seen asks maySetSeen and any other keyword maySetKeywords (RFC 8621 section 2)", async () => {
  const seenOnly = await serve([withRights("inbox", { maySetKeywords: false })]);
  expect(await mayPatch(seenOnly.client, seenOnly.store, READ)).toBe(true);
  expect(await mayPatch(seenOnly.client, seenOnly.store, FLAG)).toBe(false);
  const both: Mutation = { ...READ, patch: { ...READ.patch, ...FLAG.patch } };
  expect(await mayPatch(seenOnly.client, seenOnly.store, both)).toBe(false);
  vi.unstubAllGlobals();
  const flagsOnly = await serve([withRights("inbox", { maySetSeen: false })]);
  expect(await mayPatch(flagsOnly.client, flagsOnly.store, READ)).toBe(false);
  expect(await mayPatch(flagsOnly.client, flagsOnly.store, FLAG)).toBe(true);
});

test("every mailbox of the email has to allow the change", async () => {
  const { client, store } = await serve([
    mailbox("inbox", "inbox"),
    withRights("shared", { maySetSeen: false }),
  ]);
  expect(await mayPatch(client, store, READ)).toBe(false);
  expect(await mayPatch(client, store, FLAG)).toBe(true);
});

test("a mailbox the store does not hold is the server's to judge", async () => {
  const { client, store } = await serve([
    mailbox("inbox", "inbox"),
    withRights("shared", { maySetSeen: false }),
  ]);
  await store.commit(ACCOUNT, { mailboxes: { remove: ["shared"] } });
  expect(await mayPatch(client, store, READ)).toBe(true);
});

test("an email the store does not hold takes no change", async () => {
  const { client, store } = await serve([mailbox("inbox", "inbox")]);
  expect(await mayPatch(client, store, { ...READ, id: "gone" })).toBe(false);
});
