// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { writeFixed, writeFlowed } from "../compose/flowed-write";
import { JmapClient, JmapError } from "../jmap/client";
import { partsOf } from "./draft-parts";
import { discardDraft, writeDraft } from "./draft-write";
import { SEND_UNDO_MS, needsWrite, queueDraft, saveDraft, undoSend } from "./drafts";
import { ACCOUNT, DRAFT_FILE, draftContent, withRoles } from "./fake-jmap";
import type { FakeJmap } from "./fake-jmap";
import { IDENTITY } from "./fake-sends";
import { readIdentity } from "./identity";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { queryWindow } from "./window";

const NOW = Date.UTC(2026, 0, 1);
const ASK = { watched: [], names: { drafts: "Concepten", sent: "Verzonden" } };

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

async function serve(roles: readonly string[] = ["inbox", "drafts", "sent"]): Promise<Rig> {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  const server = withRoles(roles);
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  return rig;
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a save writes the content into the row and counts a change the server lacks", async () => {
  const { store } = await serve();
  const first = await saveDraft(store, ACCOUNT, { id: "r1", content: draftContent() });
  expect(first).toMatchObject({
    id: "r1",
    serverId: null,
    state: "draft",
    revision: 1,
    changedAt: NOW,
  });
  expect(needsWrite(first)).toBe(true);
  const second = await saveDraft(store, ACCOUNT, {
    id: "r1",
    content: draftContent({ subject: "Dinner" }),
  });
  expect(second).toMatchObject({ subject: "Dinner", revision: 2 });
  expect(await store.draft(ACCOUNT, "r1")).toEqual(second);
});

test("a row opened from the server's draft replaces that copy on its first write", async () => {
  const { server, client, store } = await serve();
  const opened = await writeAndRead({ server, client, store }, "r0");
  await saveDraft(store, ACCOUNT, {
    id: "r1",
    content: draftContent({ subject: "Edited" }),
    opened: { serverId: opened, threadId: null },
  });
  const saved = await writeDraft(client, store, "r1", ASK);
  expect(saved.status).toBe("saved");
  expect(server.emails.has(opened)).toBe(false);
});

async function writeAndRead({ client, store }: Rig, id: string): Promise<string> {
  await saveDraft(store, ACCOUNT, { id, content: draftContent() });
  const saved = await writeDraft(client, store, id, ASK);
  if (saved.status !== "saved" || saved.row.serverId === null) {
    throw new Error("the draft did not land");
  }
  return saved.row.serverId;
}

test("a write creates the draft in Drafts with $draft and $seen and lands it in the list a tab watches, rfc8621_4_6", async () => {
  const rig = await serve();
  await queryWindow(rig.client, rig.store, "drafts", 0);
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent() });
  const saved = await writeDraft(rig.client, rig.store, "r1", { ...ASK, watched: ["drafts"] });
  const serverId = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  expect(saved.status === "saved" ? saved.changes.windows : []).toContain("drafts");
  expect((await rig.store.query(ACCOUNT, "drafts"))?.pages).toEqual([{ page: 0, ids: [serverId] }]);
  const row = await rig.store.draft(ACCOUNT, "r1");
  expect(row).toMatchObject({ serverId, savedRevision: 1, savedAt: NOW, failure: null });
  expect(rig.server.emails.get(serverId)).toMatchObject({
    mailboxIds: { drafts: true },
    keywords: { $draft: true, $seen: true },
    subject: "Lunch",
  });
  const leads = rig.server.posted().map(([first]) => first?.[0]);
  expect(leads.filter((name) => name === "Email/set")).toHaveLength(1);
});

test("a write after a change replaces the copy with a new id and destroys the old one", async () => {
  const rig = await serve();
  const first = await writeAndRead(rig, "r1");
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ subject: "Later" }) });
  const second = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(second.status).toBe("saved");
  const row = await rig.store.draft(ACCOUNT, "r1");
  expect(row?.serverId).not.toBe(first);
  expect(row?.savedRevision).toBe(2);
  expect(rig.server.emails.has(first)).toBe(false);
  expect((await rig.store.emails(ACCOUNT, [first])).has(first)).toBe(false);
});

test("a row without a change sends nothing", async () => {
  const rig = await serve();
  await writeAndRead(rig, "r1");
  const before = rig.server.requests.length;
  expect(await writeDraft(rig.client, rig.store, "r1", ASK)).toEqual({ status: "clean" });
  expect(await writeDraft(rig.client, rig.store, "gone", ASK)).toEqual({ status: "clean" });
  expect(rig.server.requests).toHaveLength(before);
});

test("a refused write keeps the content and the cause in the row until a write lands", async () => {
  const rig = await serve();
  await writeAndRead(rig, "r1");
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ subject: "Later" }) });
  rig.server.createRefusal = { type: "forbidden" };
  const refused = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(refused.status).toBe("refused");
  expect(await rig.store.draft(ACCOUNT, "r1")).toMatchObject({
    subject: "Later",
    savedRevision: 1,
    failure: { stage: "save", type: "forbidden", recipients: [] },
  });
  rig.server.createRefusal = null;
  await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(await rig.store.draft(ACCOUNT, "r1")).toMatchObject({ savedRevision: 2, failure: null });
});

test("a copy a server destroyed beside a refused create leaves the row without one, rfc8620_5_3", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  const content = draftContent({ attachments: [DRAFT_FILE] });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content });
  const written = await writeDraft(rig.client, rig.store, "r1", ASK);
  const first = written.status === "saved" ? (written.row.serverId ?? "") : "";
  expect((await rig.store.draft(ACCOUNT, "r1"))?.parts).toEqual({ "upload-1": `${first}-3` });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: { ...content, subject: "Later" } });
  rig.server.createRefusal = { type: "forbidden" };
  await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(rig.server.emails.has(first)).toBe(false);
  const left = await rig.store.draft(ACCOUNT, "r1");
  expect(left).toMatchObject({ serverId: null, threadId: null });
  expect(left?.parts).toEqual({});
  rig.server.createRefusal = null;
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(saved.status).toBe("saved");
  const set = rig.server
    .posted()
    .at(-2)
    ?.find(([name]) => name === "Email/set");
  expect(set?.[1]).not.toHaveProperty("destroy");
});

test("attachments past the account's limit are refused before any request", async () => {
  const rig = await serve();
  const heavy = { ...DRAFT_FILE, size: rig.server.uploadLimit + 1 };
  await saveDraft(rig.store, ACCOUNT, {
    id: "r1",
    content: draftContent({ attachments: [heavy] }),
  });
  const before = rig.server.requests.length;
  const refused = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(refused).toMatchObject({ status: "refused", row: { failure: { type: "tooLarge" } } });
  expect(rig.server.requests).toHaveLength(before);
});

test("attachments that weigh exactly the account's limit go to the server", async () => {
  const rig = await serve();
  const full = { ...DRAFT_FILE, size: rig.server.uploadLimit };
  rig.server.uploads.set(full.blobId, { type: full.type, size: full.size });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ attachments: [full] }) });
  expect((await writeDraft(rig.client, rig.store, "r1", ASK)).status).toBe("saved");
});

test("a write before any mailbox was read finds Drafts on the server and creates none", async () => {
  const { server, client } = await serve();
  const store = new MemoryMailStore();
  await saveDraft(store, ACCOUNT, { id: "r1", content: draftContent() });
  const saved = await writeDraft(client, store, "r1", ASK);
  const serverId = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  expect(server.emails.get(serverId)?.mailboxIds).toEqual({ drafts: true });
  expect([...server.mailboxes.values()].filter((box) => box.role === "drafts")).toHaveLength(1);
});

test("a server without the vendor capability takes the text one line per paragraph", async () => {
  const rig = await serve();
  rig.server.vendor = false;
  const client = new JmapClient(ACCOUNT);
  const long = {
    depth: 0,
    text: Array.from({ length: 30 }, (_, at) => `word${String(at)}`).join(" "),
  };
  const lines = [long, { depth: 1, text: "Earlier." }];
  await saveDraft(rig.store, ACCOUNT, {
    id: "r1",
    content: draftContent({ text: writeFlowed(lines) }),
  });
  await writeDraft(client, rig.store, "r1", ASK);
  const set = rig.server
    .posted()
    .at(-1)
    ?.find(([name]) => name === "Email/set");
  expect(set?.[1]).toMatchObject({
    create: {
      draft: {
        bodyStructure: { subParts: [{ partId: "text", type: "text/plain" }, {}] },
        bodyValues: { text: { value: writeFixed(lines) } },
      },
    },
  });
});

test("after a write the files go by the parts of the server's copy, so an upload may expire", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  const logo = {
    ...DRAFT_FILE,
    blobId: "upload-2",
    name: "logo.png",
    type: "image/png",
    cid: "logo@x",
    disposition: "inline" as const,
  };
  rig.server.uploads.set(logo.blobId, { type: logo.type, size: logo.size });
  const content = draftContent({ attachments: [DRAFT_FILE, logo] });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content });
  const first = await writeDraft(rig.client, rig.store, "r1", ASK);
  const serverId = first.status === "saved" ? first.row.serverId : null;
  expect(first.status === "saved" ? first.row.parts : null).toEqual({
    "upload-2": `${serverId ?? ""}-3`,
    "upload-1": `${serverId ?? ""}-4`,
  });
  rig.server.uploads.clear();
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: { ...content, subject: "Later" } });
  const second = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(second.status).toBe("saved");
  const sent = JSON.stringify(rig.server.posted().at(-2));
  expect(sent).toContain(`${serverId ?? ""}-4`);
  expect(sent).not.toContain("upload-1");
});

test("a plain draft's files go by the parts after its one text part", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  const content = draftContent({ mode: "plain", attachments: [DRAFT_FILE] });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content });
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  const serverId = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  expect(saved.status === "saved" ? saved.row.parts : null).toEqual({
    "upload-1": `${serverId}-2`,
  });
});

test("a file whose type is not two tokens goes as plain bytes and keeps its part", async () => {
  const rig = await serve();
  const odd = { ...DRAFT_FILE, type: "text" };
  rig.server.uploads.set(odd.blobId, { type: odd.type, size: odd.size });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent({ attachments: [odd] }) });
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  const serverId = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  expect(rig.server.bodies.get(serverId)?.attachments?.[0]?.type).toBe("application/octet-stream");
  expect(saved.status === "saved" ? saved.row.parts : null).toEqual({
    "upload-1": `${serverId}-3`,
  });
});

// A rich draft's tree as a server answers it, with the leaves after the text.
function treeWith(leaves: readonly { blobId: string; type: string }[]): Record<string, unknown> {
  const text = [
    { blobId: "text", type: "text/plain" },
    { blobId: "html", type: "text/html" },
  ];
  const alternative = { blobId: null, type: "multipart/alternative", subParts: text };
  return { blobId: null, type: "multipart/mixed", subParts: [alternative, ...leaves] };
}

test("the parts are taken only from a tree whose leaves match the files in type and count", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  const content = draftContent({ attachments: [DRAFT_FILE] });
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content });
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  const serverId = saved.status === "saved" ? (saved.row.serverId ?? "") : "";
  rig.server.structures.set(serverId, treeWith([{ blobId: "p", type: "APPLICATION/PDF" }]));
  expect(await partsOf(rig.client, content, serverId)).toEqual({ "upload-1": "p" });
  rig.server.structures.set(serverId, treeWith([{ blobId: "p", type: "image/png" }]));
  expect(await partsOf(rig.client, content, serverId)).toEqual({});
  const extra = { blobId: "q", type: "text/plain" };
  rig.server.structures.set(serverId, treeWith([{ blobId: "p", type: DRAFT_FILE.type }, extra]));
  expect(await partsOf(rig.client, content, serverId)).toEqual({});
});

test("a read of the parts that does not reach the server keeps the write and names the files as given", async () => {
  const rig = await serve();
  rig.server.uploads.set(DRAFT_FILE.blobId, { type: DRAFT_FILE.type, size: DRAFT_FILE.size });
  await saveDraft(rig.store, ACCOUNT, {
    id: "r1",
    content: draftContent({ attachments: [DRAFT_FILE] }),
  });
  const fetched = rig.server.fetch;
  let calls = 0;
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) => {
    calls += 1;
    return calls > 1 ? Promise.reject(new TypeError("down")) : fetched(input, init);
  });
  const saved = await writeDraft(rig.client, rig.store, "r1", ASK);
  expect(saved).toMatchObject({ status: "saved", row: { parts: {}, savedRevision: 1 } });
});

test("a server out of reach throws and leaves the row as it was", async () => {
  const rig = await serve();
  await saveDraft(rig.store, ACCOUNT, { id: "r1", content: draftContent() });
  const before = await rig.store.draft(ACCOUNT, "r1");
  rig.server.createRefusal = { type: "serverUnavailable" };
  await expect(writeDraft(rig.client, rig.store, "r1", ASK)).rejects.toBeInstanceOf(JmapError);
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("down")));
  await expect(writeDraft(rig.client, rig.store, "r1", ASK)).rejects.toMatchObject({
    code: "unavailable",
  });
  expect(await rig.store.draft(ACCOUNT, "r1")).toEqual(before);
});

test("an account without Drafts gets one with the role and the name given, then the draft, rfc8621_2", async () => {
  const rig = await serve(["inbox"]);
  const serverId = await writeAndRead(rig, "r1");
  const drafts = [...rig.server.mailboxes.values()].find((box) => box.role === "drafts");
  expect(drafts?.name).toBe("Concepten");
  expect(rig.server.emails.get(serverId)?.mailboxIds).toEqual({ [drafts?.id ?? ""]: true });
});

test("Send queues the draft for the undo window and Undo turns it back while it waits", async () => {
  const { store } = await serve();
  await saveDraft(store, ACCOUNT, { id: "r1", content: draftContent() });
  const queued = await queueDraft(store, ACCOUNT, "r1");
  expect(queued).toMatchObject({ state: "queued", sendAfter: NOW + SEND_UNDO_MS, failure: null });
  expect(await queueDraft(store, ACCOUNT, "r1")).toBeNull();
  expect(await undoSend(store, ACCOUNT, "r1")).toMatchObject({ state: "draft", sendAfter: null });
  expect(await undoSend(store, ACCOUNT, "r1")).toBeNull();
  const row = await store.draft(ACCOUNT, "r1");
  await store.commit(ACCOUNT, {
    drafts: { put: [{ ...(row ?? queued ?? never()), state: "sending" }] },
  });
  expect(await undoSend(store, ACCOUNT, "r1")).toBeNull();
  expect(await queueDraft(store, ACCOUNT, "gone")).toBeNull();
});

test("Send clears the cause an earlier failure left", async () => {
  const { store } = await serve();
  const row = await saveDraft(store, ACCOUNT, { id: "r1", content: draftContent() });
  const failure = { stage: "send" as const, type: "tooLarge", recipients: [] };
  await store.commit(ACCOUNT, { drafts: { put: [{ ...row, failure }] } });
  expect(await queueDraft(store, ACCOUNT, "r1")).toMatchObject({ state: "queued", failure: null });
});

function never(): never {
  throw new Error("no row");
}

test("a discard destroys the server's copy and the row, which it hands back for Undo", async () => {
  const rig = await serve();
  const serverId = await writeAndRead(rig, "r1");
  const discarded = await discardDraft(rig.client, rig.store, "r1", []);
  expect(discarded).toMatchObject({ status: "discarded", row: { id: "r1", serverId } });
  expect(rig.server.emails.has(serverId)).toBe(false);
  expect(await rig.store.draft(ACCOUNT, "r1")).toBeNull();
  expect(await discardDraft(rig.client, rig.store, "r1", [])).toBeNull();
});

test("a discard the server cannot run now throws and keeps the row", async () => {
  const rig = await serve();
  const serverId = await writeAndRead(rig, "r1");
  rig.server.refused.set(serverId, "serverUnavailable");
  await expect(discardDraft(rig.client, rig.store, "r1", [])).rejects.toMatchObject({
    code: "unavailable",
  });
  expect(await rig.store.draft(ACCOUNT, "r1")).not.toBeNull();
});

test("a discard of a copy the server lacks removes the row all the same", async () => {
  const rig = await serve();
  const serverId = await writeAndRead(rig, "r1");
  rig.server.emails.delete(serverId);
  expect((await discardDraft(rig.client, rig.store, "r1", []))?.status).toBe("discarded");
  expect(await rig.store.draft(ACCOUNT, "r1")).toBeNull();
});

test("a draft never written is discarded without a request and a refused destroy keeps the row", async () => {
  const rig = await serve();
  await saveDraft(rig.store, ACCOUNT, { id: "local", content: draftContent() });
  const before = rig.server.requests.length;
  expect((await discardDraft(rig.client, rig.store, "local", []))?.status).toBe("discarded");
  expect(rig.server.requests).toHaveLength(before);
  const serverId = await writeAndRead(rig, "r1");
  rig.server.refused.set(serverId, "forbidden");
  expect(await discardDraft(rig.client, rig.store, "r1", [])).toMatchObject({
    status: "refused",
    type: "forbidden",
  });
  expect(await rig.store.draft(ACCOUNT, "r1")).not.toBeNull();
});

test("the identity is the one whose address is the account's, else the first, rfc8621_6", async () => {
  const { client, server } = await serve();
  server.identities = [{ ...IDENTITY, id: "identity-0", email: "team@example.test" }, IDENTITY];
  expect(await readIdentity(client, "SANNE@example.test")).toEqual({
    id: "identity-1",
    name: "",
    email: "sanne@example.test",
  });
  expect((await readIdentity(client, "other@example.test"))?.id).toBe("identity-0");
  server.submission = false;
  expect(await readIdentity(new JmapClient(ACCOUNT), "sanne@example.test")).toBeNull();
});
