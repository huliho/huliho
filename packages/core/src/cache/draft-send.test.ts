// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { drainDrafts } from "./draft-send";
import { writeDraft } from "./draft-write";
import { SEND_UNDO_MS, queueDraft, saveDraft } from "./drafts";
import { ACCOUNT, DRAFT_FILE, at, draftContent, email, mailbox, withRoles } from "./fake-jmap";
import type { FakeJmap } from "./fake-jmap";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { queryWindow } from "./window";

const NOW = Date.UTC(2026, 0, 1);
const ASK = { watched: [], names: { drafts: "Concepten", sent: "Verzonden" } };
// How far apart two sends of one test are queued.
const QUEUED_APART_MS = 1000;

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

// Inbox, Drafts and Sent with the parent p1 in the inbox and in the store;
// `knobs` changes the server before anything is read.
async function serve(
  roles: readonly string[] = ["inbox", "drafts", "sent"],
  knobs: (server: FakeJmap) => void = () => undefined,
): Promise<Rig> {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  const server = withRoles(roles);
  knobs(server);
  server.addEmail(email("p1", { receivedAt: at(1) }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  return rig;
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// A reply to p1 written to the server and queued.
async function queued({ client, store }: Rig, id = "r1"): Promise<string> {
  const content = draftContent({ kind: "reply", parentId: "p1", inReplyTo: ["p1@example.test"] });
  await saveDraft(store, ACCOUNT, { id, content });
  const saved = await writeDraft(client, store, id, ASK);
  await queueDraft(store, ACCOUNT, id);
  return saved.status === "saved" ? (saved.row.serverId ?? "") : "";
}

function past(): void {
  vi.setSystemTime(NOW + SEND_UNDO_MS);
}

test("a queued draft waits out its window and then goes out once", async () => {
  const rig = await serve();
  await queued(rig);
  expect(await drainDrafts(rig.client, rig.store, ASK)).toMatchObject({ outcomes: [] });
  expect(rig.server.submissions).toEqual([]);
  past();
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect(drained.outcomes).toEqual([{ id: "r1", status: "sent", filed: true }]);
  expect(rig.server.submissions).toHaveLength(1);
  expect(await drainDrafts(rig.client, rig.store, ASK)).toMatchObject({ outcomes: [] });
  expect(rig.server.submissions).toHaveLength(1);
});

test("a send files the draft in Sent without $draft and leaves the row, rfc8621_7_5", async () => {
  const rig = await serve();
  const serverId = await queued(rig);
  past();
  await drainDrafts(rig.client, rig.store, ASK);
  expect(rig.server.submissions[0]).toMatchObject({ identityId: "identity-1", emailId: serverId });
  expect(rig.server.emails.get(serverId)).toMatchObject({
    mailboxIds: { sent: true },
    keywords: { $seen: true },
  });
  expect(await rig.store.draft(ACCOUNT, "r1")).toBeNull();
});

test("the parent of a sent reply takes $answered and of a forward $forwarded through the log", async () => {
  const rig = await serve();
  await queued(rig);
  past();
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect((await rig.store.pending(ACCOUNT)).map((row) => row.patch)).toEqual([
    { "keywords/$answered": true },
  ]);
  expect(drained.changes.threads).toContain("t-p1");
  const forward = draftContent({ kind: "forward", parentId: "p1" });
  await saveDraft(rig.store, ACCOUNT, { id: "f1", content: forward });
  await queueDraft(rig.store, ACCOUNT, "f1");
  vi.setSystemTime(NOW + 2 * SEND_UNDO_MS);
  await drainDrafts(rig.client, rig.store, ASK);
  expect((await rig.store.pending(ACCOUNT)).at(-1)?.patch).toEqual({ "keywords/$forwarded": true });
});

test.each([
  [{ type: "invalidRecipients", invalidRecipients: ["mo@example.test"] }, ["mo@example.test"]],
  [{ type: "tooLarge" }, []],
  [{ type: "forbiddenFrom" }, []],
  [{ type: "forbiddenMailFrom" }, []],
  [{ type: "forbiddenToSend" }, []],
  [{ type: "tooManyRecipients" }, []],
  [{ type: "noRecipients" }, []],
  [{ type: "invalidEmail" }, []],
  [{ type: "serverFail" }, []],
])(
  "a refusal %j turns the row back into a draft with its cause, rfc8621_7_5",
  async (refusal, recipients) => {
    const rig = await serve();
    const serverId = await queued(rig);
    past();
    rig.server.sendRefusal = refusal;
    const drained = await drainDrafts(rig.client, rig.store, ASK);
    const failure = { stage: "send", type: refusal.type, recipients };
    expect(drained.outcomes).toEqual([{ id: "r1", status: "refused", failure }]);
    expect(await rig.store.draft(ACCOUNT, "r1")).toMatchObject({
      state: "draft",
      sendAfter: null,
      failure,
      serverId,
    });
    expect(rig.server.emails.get(serverId)?.mailboxIds).toEqual({ drafts: true });
  },
);

test("a server out of reach keeps the draft queued and ends the round", async () => {
  const rig = await serve();
  await queued(rig, "r1");
  await queued(rig, "r2");
  past();
  rig.server.sendRefusal = { type: "serverUnavailable" };
  const refused = await drainDrafts(rig.client, rig.store, ASK);
  expect(refused.outcomes).toEqual([{ id: "r1", status: "waiting" }]);
  expect((await rig.store.draft(ACCOUNT, "r1"))?.state).toBe("queued");
  rig.server.sendRefusal = null;
  rig.server.sendMethodError = "serverUnavailable";
  expect((await drainDrafts(rig.client, rig.store, ASK)).outcomes).toEqual([
    { id: "r1", status: "waiting" },
  ]);
  rig.server.sendMethodError = null;
  const sent = await drainDrafts(rig.client, rig.store, ASK);
  expect(sent.outcomes.map((one) => one.status)).toEqual(["sent", "sent"]);
});

test("a send whose answer was lost is read from the server before it goes again", async () => {
  const rig = await serve();
  const serverId = await queued(rig);
  past();
  const fetched = rig.server.fetch;
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) =>
    fetched(input, init).then((response) => {
      if (rig.server.submissions.length > 0) {
        throw new TypeError("the answer was lost");
      }
      return response;
    }),
  );
  expect((await drainDrafts(rig.client, rig.store, ASK)).outcomes).toEqual([
    { id: "r1", status: "waiting" },
  ]);
  expect((await rig.store.draft(ACCOUNT, "r1"))?.state).toBe("sending");
  vi.stubGlobal("fetch", fetched);
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect(drained.outcomes).toEqual([{ id: "r1", status: "sent", filed: true }]);
  expect(rig.server.submissions).toHaveLength(1);
  expect(rig.server.emails.get(serverId)?.mailboxIds).toEqual({ sent: true });
});

test("a sending row whose copy is still a draft goes out again", async () => {
  const rig = await serve();
  await queued(rig);
  past();
  const row = await rig.store.draft(ACCOUNT, "r1");
  await rig.store.commit(ACCOUNT, {
    drafts: { put: row === null ? [] : [{ ...row, state: "sending" }] },
  });
  expect((await drainDrafts(rig.client, rig.store, ASK)).outcomes).toEqual([
    { id: "r1", status: "sent", filed: true },
  ]);
  expect(rig.server.submissions).toHaveLength(1);
});

test("a row changed since its save is written first and the send names the new copy", async () => {
  const rig = await serve();
  const first = await queued(rig);
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ subject: "Changed" }) });
  past();
  await drainDrafts(rig.client, rig.store, ASK);
  const [submission] = rig.server.submissions;
  expect(submission?.["emailId"]).not.toBe(first);
  expect(rig.server.emails.has(first)).toBe(false);
  const sent = rig.server.emails.get(String(submission?.["emailId"]));
  expect(sent?.subject).toBe("Changed");
});

test("an account without Sent gets one, and a draft the server would not file is sent all the same", async () => {
  const rig = await serve(["inbox", "drafts"]);
  const serverId = await queued(rig);
  past();
  rig.server.refused.set(serverId, "forbidden");
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect(drained.outcomes).toEqual([{ id: "r1", status: "sent", filed: false }]);
  const sent = [...rig.server.mailboxes.values()].find((box) => box.role === "sent");
  expect(sent?.name).toBe("Verzonden");
});

test("the queue goes out in the order the drafts were sent, rfc8621_7", async () => {
  const rig = await serve();
  await saveDraft(rig.store, ACCOUNT, { id: "a", content: draftContent() });
  await saveDraft(rig.store, ACCOUNT, { id: "b", content: draftContent() });
  await queueDraft(rig.store, ACCOUNT, "b");
  vi.setSystemTime(NOW + QUEUED_APART_MS);
  await queueDraft(rig.store, ACCOUNT, "a");
  vi.setSystemTime(NOW + QUEUED_APART_MS + SEND_UNDO_MS);
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect(drained.outcomes.map((one) => one.id)).toEqual(["b", "a"]);
});

test("a parent in a mailbox that takes no keyword gets no $answered, rfc8621_2", async () => {
  const rig = await serve(undefined, (server) => {
    const inbox = mailbox("inbox", "inbox");
    server.putMailbox({ ...inbox, myRights: { ...inbox.myRights, maySetKeywords: false } });
  });
  await queued(rig);
  past();
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  expect(drained.outcomes).toEqual([{ id: "r1", status: "sent", filed: true }]);
  expect(await rig.store.pending(ACCOUNT)).toEqual([]);
});

test("a failure other than the network or a server out of reach leaves the round", async () => {
  const rig = await serve();
  await queued(rig);
  past();
  rig.server.queue.push(Response.json({ error: "upstream_credentials" }, { status: 502 }));
  await expect(drainDrafts(rig.client, rig.store, ASK)).rejects.toMatchObject({
    code: "credentials",
  });
  expect(rig.server.submissions).toEqual([]);
});

test("a queued draft whose save the server refuses turns back into a draft with that cause", async () => {
  const rig = await serve();
  await queued(rig);
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ subject: "Changed" }) });
  past();
  rig.server.createRefusal = { type: "overQuota" };
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  const failure = { stage: "save", type: "overQuota", recipients: [] };
  expect(drained.outcomes).toEqual([{ id: "r1", status: "refused", failure }]);
  expect(await rig.store.draft(ACCOUNT, "r1")).toMatchObject({ state: "draft", failure });
  expect(rig.server.submissions).toEqual([]);
});

test("a queued draft whose copy the server destroyed beside a refused save keeps no server copy, rfc8620_5_3", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  const content = draftContent({ attachments: [DRAFT_FILE] });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content });
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  const first = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  expect((await rig.store.draft(ACCOUNT, "r1"))?.parts).toEqual({ "upload-1": `${first}-3` });
  await queueDraft(rig.store, ACCOUNT, "r1");
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: { ...content, subject: "Changed" } });
  past();
  rig.server.createRefusal = { type: "overQuota" };
  const drained = await drainDrafts(rig.client, rig.store, ASK);
  const failure = { stage: "save", type: "overQuota", recipients: [] };
  expect(drained.outcomes).toEqual([{ id: "r1", status: "refused", failure }]);
  expect(rig.server.emails.has(first)).toBe(false);
  const left = await rig.store.draft(ACCOUNT, "r1");
  expect(left).toMatchObject({
    state: "draft",
    sendAfter: null,
    failure,
    serverId: null,
    threadId: null,
    subject: "Changed",
  });
  expect(left?.parts).toEqual({});
  rig.server.createRefusal = null;
  expect((await writeDraft(rig.client, rig.store, "r1", ASK)).status).toBe("saved");
});

test("an account that takes no send refuses before anything goes out", async () => {
  const rig = await serve();
  await queued(rig);
  past();
  rig.server.submission = false;
  const drained = await drainDrafts(new JmapClient(ACCOUNT), rig.store, ASK);
  expect(drained.outcomes).toEqual([
    {
      id: "r1",
      status: "refused",
      failure: { stage: "send", type: "forbiddenToSend", recipients: [] },
    },
  ]);
  expect(rig.server.submissions).toEqual([]);
});
