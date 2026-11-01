// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, json, mailbox } from "./fake-jmap";
import { setMailbox } from "./mailbox-set";
import type { MailboxRefusal, MailboxUpdate, MailboxWrite } from "./mailbox-set";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { visibleMailboxes } from "./visible";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

// A folder the user made, holding every right but submission.
function folder(id: string, parentId: string | null = null): Mailbox {
  const plain = mailbox(id);
  return {
    ...plain,
    parentId,
    myRights: {
      ...plain.myRights,
      mayAddItems: true,
      mayRemoveItems: true,
      mayCreateChild: true,
      mayRename: true,
      mayDelete: true,
    },
  };
}

// INBOX and Archive as `mailbox` makes them; the folder work with plans
// under it, the folder old with o1 alone and o2 that INBOX holds too,
// and plain, which takes no change.
function serve(): Rig {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("archive", "archive"));
  server.putMailbox(folder("work"));
  server.putMailbox(folder("plans", "work"));
  server.putMailbox(folder("old"));
  server.putMailbox(mailbox("plain"));
  server.addEmail(email("o1", { mailboxIds: ["old"], receivedAt: at(1) }));
  server.addEmail(email("o2", { mailboxIds: ["old", "inbox"], receivedAt: at(2) }));
  server.addEmail(email("i1", { receivedAt: at(3) }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

// The rig with the tree in the store and no request recorded yet.
async function synced(): Promise<Rig> {
  const rig = serve();
  await syncMailboxes(rig.client, rig.store);
  rig.server.requests.length = 0;
  return rig;
}

async function rowOf(store: MemoryMailStore, id: string): Promise<Mailbox | undefined> {
  return (await store.mailboxes(ACCOUNT)).find((row) => row.id === id);
}

// The names of the calls each request carried.
function sent(server: FakeJmap): string[][] {
  return server.posted().map((calls) => calls.map(([name]) => name));
}

// An answer the server gives by hand: the write's own, then the mailbox
// changes from the state the store holds with nothing in them.
function canned(server: FakeJmap, set: [string, Record<string, unknown>]): void {
  const state = String(server.sequence);
  const none = { accountId: UPSTREAM, state, list: [], notFound: [] };
  const changes = {
    accountId: UPSTREAM,
    oldState: state,
    newState: state,
    hasMoreChanges: false,
    created: [],
    updated: [],
    destroyed: [],
  };
  server.queue.push(
    json(200, {
      sessionState: server.sessionState,
      methodResponses: [
        [...set, "s"],
        ["Mailbox/changes", changes, "c:Mailbox"],
        ["Mailbox/get", none, "n:Mailbox"],
        ["Mailbox/get", none, "u:Mailbox"],
      ],
    }),
  );
}

const TRIPS = { name: "Trips", parentId: null };

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a write past maxObjectsInSet is refused before anything is sent, one at the limit goes out", async () => {
  const { server, client, store } = serve();
  server.maxObjectsInSet = 1;
  await syncMailboxes(client, store);
  server.requests.length = 0;
  const write = { create: { c1: TRIPS }, destroy: ["nope"] };
  await expect(setMailbox(client, store, write, [])).rejects.toMatchObject({
    code: "limit",
    limit: "maxObjectsInSet",
  });
  expect(server.requests).toHaveLength(0);
  await setMailbox(client, store, { create: { c1: TRIPS } }, []);
  expect(sent(server)[0]?.[0]).toBe("Mailbox/set");
});

// One refused write: what it is, the write, what comes back and how the
// server stands before it.
type Refused = [
  string,
  MailboxWrite,
  Record<string, MailboxRefusal>,
  ((server: FakeJmap) => void)?,
];

function invalid(key: string, property: string): Record<string, MailboxRefusal> {
  return { [key]: { type: "invalidProperties", properties: [property] } };
}

function refused(key: string, type: string): Record<string, MailboxRefusal> {
  return { [key]: { type, properties: [] } };
}

// A write of one create under the creation id c1.
function create(name: string, parentId: string | null = null, role?: string): MailboxWrite {
  return { create: { c1: role === undefined ? { name, parentId } : { name, parentId, role } } };
}

function update(id: string, patch: MailboxUpdate): MailboxWrite {
  return { update: { [id]: patch } };
}

function destroy(id: string): MailboxWrite {
  return { destroy: [id], onDestroyRemoveEmails: true };
}

function shortNames(server: FakeJmap): void {
  server.maxSizeMailboxName = 4;
}

function noTopLevel(server: FakeJmap): void {
  server.topLevel = false;
}

function renamableInbox(server: FakeJmap): void {
  server.putMailbox({ ...folder("inbox"), role: "inbox" });
}

function overQuota(server: FakeJmap): void {
  server.folderRefusal = "overQuota";
}

const ALL_THREE = { create: { c1: TRIPS }, update: { work: { name: "Jobs" } }, destroy: ["old"] };

const REFUSALS: Refused[] = [
  ["a sibling's name", create("work"), invalid("c1", "name")],
  ["a name with white space at its end", create("Trips "), invalid("c1", "name")],
  ["an empty name", create(""), invalid("c1", "name")],
  ["a name past the account's limit", create("Trips"), invalid("c1", "name"), shortNames],
  ["a parent the account lacks", create("Trips", "nope"), invalid("c1", "parentId")],
  ["an unknown creation id as the parent", create("Trips", "#c9"), invalid("c1", "parentId")],
  ["a parent without the right to a child", create("Trips", "plain"), refused("c1", "forbidden")],
  [
    "a top-level folder the account refuses",
    create("Trips"),
    refused("c1", "forbidden"),
    noTopLevel,
  ],
  ["a role another mailbox holds", create("Old", null, "archive"), invalid("c1", "role")],
  ["a role outside the six", update("work", { role: "box" }), invalid("work", "role")],
  ["the inbox role elsewhere", update("work", { role: "inbox" }), invalid("work", "role")],
  [
    "a rename of INBOX",
    update("inbox", { name: "Post" }),
    refused("inbox", "forbidden"),
    renamableInbox,
  ],
  ["a rename without the right", update("plain", { name: "Other" }), refused("plain", "forbidden")],
  ["a parent under the folder", update("work", { parentId: "plans" }), invalid("work", "parentId")],
  ["a rename onto a sibling's name", update("old", { name: "work" }), invalid("old", "name")],
  [
    "a role mailbox hidden",
    update("archive", { isSubscribed: false }),
    refused("archive", "forbidden"),
  ],
  ["an update of an unknown id", update("nope", { name: "Other" }), refused("nope", "notFound")],
  ["a destroy of a role mailbox", destroy("archive"), refused("archive", "forbidden")],
  ["a destroy without the right", destroy("plain"), refused("plain", "forbidden")],
  ["a destroy of a folder with a subfolder", destroy("work"), refused("work", "mailboxHasChild")],
  [
    "a destroy of mail without the argument",
    { destroy: ["old"] },
    refused("old", "mailboxHasEmail"),
  ],
  ["a destroy of an unknown id", destroy("nope"), refused("nope", "notFound")],
  [
    "the server's own refusal",
    ALL_THREE,
    {
      ...refused("c1", "overQuota"),
      ...refused("work", "overQuota"),
      ...refused("old", "overQuota"),
    },
    overQuota,
  ],
];

test.each(REFUSALS)(
  "%s comes back as its SetError with nothing written (RFC 8621 section 2.5)",
  async (_, write, expected, setup) => {
    const { server, client, store } = serve();
    setup?.(server);
    await syncMailboxes(client, store);
    const before = await store.mailboxes(ACCOUNT);
    const done = await setMailbox(client, store, write, []);
    expect({ ...done.notCreated, ...done.notUpdated, ...done.notDestroyed }).toEqual(expected);
    expect([done.created, done.updated, done.destroyed]).toEqual([{}, [], []]);
    expect(await store.mailboxes(ACCOUNT)).toEqual(before);
  },
);

test("an object the answer names nowhere reads as a server failure", async () => {
  const { server, client, store } = await synced();
  canned(server, ["Mailbox/set", { accountId: UPSTREAM, newState: String(server.sequence) }]);
  const done = await setMailbox(
    client,
    store,
    { create: { c1: TRIPS }, update: { work: { name: "Jobs" } }, destroy: ["old"] },
    [],
  );
  const failed = { type: "serverFail", properties: [] };
  expect(done).toMatchObject({
    notCreated: { c1: failed },
    notUpdated: { work: failed },
    notDestroyed: { old: failed },
  });
});

test("a call refused whole throws: serverUnavailable as an unreachable server, any other type as the method's failure", async () => {
  const { server, client, store } = await synced();
  const write = { create: { c1: TRIPS } };
  canned(server, ["error", { type: "serverUnavailable" }]);
  await expect(setMailbox(client, store, write, [])).rejects.toMatchObject({
    name: "JmapError",
    code: "unavailable",
  });
  canned(server, ["error", { type: "forbidden" }]);
  await expect(setMailbox(client, store, write, [])).rejects.toMatchObject({
    name: "MethodFailure",
    type: "forbidden",
  });
  server.readOnly = true;
  await expect(setMailbox(client, store, write, [])).rejects.toMatchObject({
    type: "accountReadOnly",
  });
  expect(await rowOf(store, "mb-new-1")).toBeUndefined();
});

test("a create leads one request with the mailbox changes behind it, so the folder lands under the id the server gave it (RFC 8620 section 3.7)", async () => {
  const { server, client, store } = await synced();
  const done = await setMailbox(
    client,
    store,
    { create: { c1: { name: "Receipts", parentId: "work" } } },
    [],
  );
  expect(sent(server)).toEqual([["Mailbox/set", "Mailbox/changes", "Mailbox/get", "Mailbox/get"]]);
  expect(server.posted()[0]?.[0]?.[1]).toEqual({
    accountId: UPSTREAM,
    ifInState: null,
    create: { c1: { name: "Receipts", parentId: "work" } },
  });
  expect(done.created).toEqual({ c1: "mb-new-1" });
  expect(done.notCreated).toEqual({});
  expect(await rowOf(store, "mb-new-1")).toMatchObject({
    name: "Receipts",
    parentId: "work",
    isSubscribed: true,
  });
  expect(done.changes.mailboxes).toBe(true);
});

test("a subfolder names the folder the same write creates by its creation id (RFC 8620 section 5.3)", async () => {
  const { client, store } = await synced();
  const done = await setMailbox(
    client,
    store,
    { create: { c2: { name: "2026", parentId: "#c1" }, c1: TRIPS } },
    [],
  );
  expect(done.created).toEqual({ c1: "mb-new-1", c2: "mb-new-2" });
  expect(await rowOf(store, "mb-new-2")).toMatchObject({ name: "2026", parentId: "mb-new-1" });
});

test("a rename and a move under another parent keep the id", async () => {
  const { client, store } = await synced();
  const done = await setMailbox(
    client,
    store,
    { update: { work: { name: "Jobs" }, old: { parentId: "work" } } },
    [],
  );
  expect(done.updated).toEqual(["work", "old"]);
  expect(done.notUpdated).toEqual({});
  expect(await rowOf(store, "work")).toMatchObject({ name: "Jobs", parentId: null });
  expect(await rowOf(store, "old")).toMatchObject({ name: "old", parentId: "work" });
});

test("hide takes a folder out of the tree's mailboxes and show brings it back", async () => {
  const { client, store } = await synced();
  await setMailbox(client, store, { update: { old: { isSubscribed: false } } }, []);
  expect(await rowOf(store, "old")).toMatchObject({ isSubscribed: false });
  const hidden = visibleMailboxes(await store.mailboxes(ACCOUNT)).map(({ id }) => id);
  expect(hidden).not.toContain("old");
  await setMailbox(client, store, { update: { old: { isSubscribed: true } } }, []);
  const shown = visibleMailboxes(await store.mailboxes(ACCOUNT)).map(({ id }) => id);
  expect(shown).toContain("old");
});

test("a role moves in one write, the mailbox that held it letting go first (RFC 8621 section 2)", async () => {
  const { client, store } = await synced();
  const done = await setMailbox(
    client,
    store,
    { update: { archive: { role: null }, old: { role: "archive" } } },
    [],
  );
  expect(done.updated).toEqual(["archive", "old"]);
  expect(await rowOf(store, "archive")).toMatchObject({ role: null });
  expect(await rowOf(store, "old")).toMatchObject({ role: "archive" });
});

test("a destroy with its mail takes the folder, its list, the emails it alone held and their threads out of the store, while another tab still shows the folder; an email another folder holds stays there", async () => {
  const { server, client, store } = await synced();
  await queryWindow(client, store, "old", 0);
  await queryWindow(client, store, "inbox", 0);
  server.requests.length = 0;
  const write = { destroy: ["old"], onDestroyRemoveEmails: true };
  const done = await setMailbox(client, store, write, ["inbox", "old"]);
  expect(sent(server)[0]?.slice(0, 2)).toEqual(["Mailbox/set", "Mailbox/changes"]);
  expect(sent(server)[0]).toContain("Email/changes");
  expect(server.posted()[0]?.[0]?.[1]).toMatchObject({
    destroy: ["old"],
    onDestroyRemoveEmails: true,
  });
  expect(done.destroyed).toEqual(["old"]);
  expect(done.notDestroyed).toEqual({});
  expect(await rowOf(store, "old")).toBeUndefined();
  expect(await store.query(ACCOUNT, "old")).toBeNull();
  expect((await store.emails(ACCOUNT, ["o1"])).size).toBe(0);
  expect((await store.threads(ACCOUNT, ["t-o1"])).size).toBe(0);
  expect((await store.emails(ACCOUNT, ["o2"])).get("o2")?.mailboxIds).toEqual({ inbox: true });
  expect(done.changes.windows).toContain("old");
  const inbox = await queryWindow(client, store, "inbox", 0);
  expect(inbox.ids).toEqual(["i1", "o2"]);
});

test("a store that holds no tree fetches it before the write", async () => {
  const { server, client, store } = serve();
  await setMailbox(client, store, { create: { c1: TRIPS } }, []);
  expect(sent(server)).toEqual([
    ["Mailbox/get"],
    ["Mailbox/set", "Mailbox/changes", "Mailbox/get", "Mailbox/get"],
  ]);
  expect(await rowOf(store, "mb-new-1")).toMatchObject({ name: "Trips" });
});

test("a write behind states past the server's horizon still lands: the tree comes whole and the emails and lists drop for a refetch (RFC 8620 section 5.2)", async () => {
  const { server, client, store } = await synced();
  await queryWindow(client, store, "inbox", 0);
  server.addEmail(email("i2", { receivedAt: at(4) }));
  server.forget();
  const done = await setMailbox(client, store, { create: { c1: TRIPS } }, ["inbox"]);
  expect(done.created).toEqual({ c1: "mb-new-1" });
  expect(await rowOf(store, "mb-new-1")).toMatchObject({ name: "Trips" });
  expect(await store.query(ACCOUNT, "inbox")).toBeNull();
  expect(done.changes.windows).toContain("inbox");
});

test("a write whose changes take more than one round sends its Mailbox/set once, in front of the first round", async () => {
  const { server, client, store } = await synced();
  server.changesCap = 1;
  server.putMailbox(folder("notes"));
  server.putMailbox(folder("bills"));
  const done = await setMailbox(client, store, { create: { c1: TRIPS } }, []);
  expect(sent(server)).toEqual([
    ["Mailbox/set", "Mailbox/changes", "Mailbox/get", "Mailbox/get"],
    ["Mailbox/changes", "Mailbox/get", "Mailbox/get"],
    ["Mailbox/changes", "Mailbox/get", "Mailbox/get"],
  ]);
  expect(done.created).toEqual({ c1: "mb-new-1" });
  expect(await rowOf(store, "mb-new-1")).toMatchObject({ name: "Trips" });
  expect(await store.state(ACCOUNT, "Mailbox")).toBe(String(server.sequence));
});
