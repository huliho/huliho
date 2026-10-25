// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { at, email, mailbox, quota } from "./fake-jmap";
import { MemoryMailStore } from "./memory";
import type { EmailBody, PendingRow, QueryRow, SearchRow } from "./store";

const ROW: QueryRow = {
  id: "inbox",
  queryState: "3",
  total: 1,
  pages: [{ page: 0, ids: ["e1"] }],
  pending: [],
  fresh: null,
};

const PART = {
  partId: "1",
  blobId: "b1",
  size: 7,
  name: null,
  type: "text/plain",
  charset: "utf-8",
  disposition: null,
  cid: null,
  language: null,
  location: null,
};

const BODY: EmailBody = {
  id: "e1",
  bodyStructure: PART,
  textBody: [PART],
  htmlBody: [PART],
  attachments: [],
  bodyValues: { "1": { value: "Hello e", isEncodingProblem: false, isTruncated: false } },
  authentication: { status: "absent" },
  flowed: null,
  large: false,
  fetchedAt: 5,
  bytes: 7,
};

const CHANGE: PendingRow = {
  seq: 1,
  type: "Email",
  id: "e1",
  patch: { "keywords/$seen": true },
  inverse: { "keywords/$seen": null },
  sentAt: null,
};

test("rows belong to one account and come back as copies", async () => {
  const store = new MemoryMailStore();
  const header = email("e1", { receivedAt: at(1) });
  await store.commit("acc-1", {
    mailboxes: { put: [mailbox("inbox", "inbox")] },
    emails: { put: [header] },
    threads: { put: [{ id: "t-e1", emailIds: ["e1"], members: {} }] },
    queries: { put: [ROW] },
    states: { Email: "3", Thread: "3" },
  });
  expect(await store.mailboxes("acc-2")).toEqual([]);
  expect(await store.state("acc-2", "Email")).toBeNull();
  expect(await store.query("acc-2", "inbox")).toBeNull();
  header.subject = "changed after the commit";
  const read = (await store.emails("acc-1", ["e1", "e9"])).get("e1");
  expect(read?.subject).toBe("Message e1");
  if (read !== undefined) {
    read.subject = "changed after the read";
  }
  expect((await store.emails("acc-1", ["e1"])).get("e1")?.subject).toBe("Message e1");
  const row = await store.query("acc-1", "inbox");
  row?.pages.push({ page: 1, ids: ["e2"] });
  expect((await store.query("acc-1", "inbox"))?.pages).toHaveLength(1);
  expect((await store.threads("acc-1", ["t-e1"])).get("t-e1")?.emailIds).toEqual(["e1"]);
  expect(await store.queries("acc-1")).toEqual([ROW]);
});

test("a batch empties the named areas first, then removes, then puts, then moves the states", async () => {
  const store = new MemoryMailStore();
  await store.commit("acc-1", {
    mailboxes: { put: [mailbox("inbox", "inbox"), mailbox("sent", "sent")] },
    emails: { put: [email("e1", { receivedAt: at(1) }), email("e2", { receivedAt: at(2) })] },
    queries: { put: [ROW] },
    states: { Mailbox: "1", Email: "1", Thread: "1" },
  });
  await store.commit("acc-1", {
    reset: ["emails", "queries"],
    mailboxes: { remove: ["sent"], put: [mailbox("drafts", "drafts")] },
    emails: { put: [email("e3", { receivedAt: at(3) })] },
    states: { Email: null, Thread: "2" },
  });
  expect((await store.mailboxes("acc-1")).map((row) => row.id)).toEqual(["inbox", "drafts"]);
  expect([...(await store.emails("acc-1", ["e1", "e2", "e3"])).keys()]).toEqual(["e3"]);
  expect(await store.queries("acc-1")).toEqual([]);
  expect(await store.state("acc-1", "Mailbox")).toBe("1");
  expect(await store.state("acc-1", "Email")).toBeNull();
  expect(await store.state("acc-1", "Thread")).toBe("2");
});

test("a body and the pending changes belong to one account, come back as copies and leave by a batch", async () => {
  const store = new MemoryMailStore();
  await store.commit("acc-1", {
    bodies: { put: [BODY] },
    pending: { put: [{ ...CHANGE, seq: 2 }, CHANGE] },
  });
  expect(await store.body("acc-2", "e1")).toBeNull();
  expect(await store.pending("acc-2")).toEqual([]);
  const read = await store.body("acc-1", "e1");
  expect(read).toEqual(BODY);
  if (read !== null) {
    read.large = true;
  }
  expect((await store.body("acc-1", "e1"))?.large).toBe(false);
  // The log reads oldest first, whatever order its rows were written in.
  expect((await store.pending("acc-1")).map((row) => row.seq)).toEqual([1, 2]);
  expect(await store.bodySizes()).toEqual([
    { accountId: "acc-1", id: "e1", fetchedAt: 5, bytes: 7 },
  ]);
  await store.commit("acc-1", { bodies: { remove: ["e1"] }, pending: { remove: [1] } });
  expect(await store.body("acc-1", "e1")).toBeNull();
  expect((await store.pending("acc-1")).map((row) => row.seq)).toEqual([2]);
  await store.commit("acc-1", { reset: ["pending", "bodies"], bodies: { put: [BODY] } });
  expect(await store.pending("acc-1")).toEqual([]);
  expect(await store.bodySizes()).toHaveLength(1);
});

test("the quotas belong to one account, come back as a copy and leave on null", async () => {
  const store = new MemoryMailStore();
  const held = { state: "4", list: [quota("q1")] };
  await store.commit("acc-1", { quota: held });
  expect(await store.quota("acc-2")).toBeNull();
  const read = await store.quota("acc-1");
  expect(read).toEqual(held);
  read?.list.pop();
  expect((await store.quota("acc-1"))?.list).toHaveLength(1);
  await store.commit("acc-1", { states: { Mailbox: "5" } });
  expect(await store.quota("acc-1")).toEqual(held);
  await store.commit("acc-1", { quota: null });
  expect(await store.quota("acc-1")).toBeNull();
});

test("the store names the accounts that hold a row and forgets every row at once", async () => {
  const store = new MemoryMailStore();
  await store.mailboxes("acc-0");
  expect(await store.accounts()).toEqual([]);
  await store.commit("acc-1", { states: { Email: "1" } });
  await store.commit("acc-2", { bodies: { put: [BODY] } });
  await store.commit("acc-3", { quota: { state: "1", list: [] } });
  expect(await store.accounts()).toEqual(["acc-1", "acc-2", "acc-3"]);
  await store.destroy();
  expect(await store.accounts()).toEqual([]);
  expect(await store.body("acc-2", "e1")).toBeNull();
  expect(await store.state("acc-1", "Email")).toBeNull();
});

const SEARCH: SearchRow = {
  id: '{"q":"invoice"}',
  queryState: "3",
  total: 1,
  pages: [{ page: 0, ids: ["e1"] }],
  run: 1,
};

test("a search belongs to one account, comes back as a copy and leaves by a batch or a reset", async () => {
  const store = new MemoryMailStore();
  await store.commit("acc-1", { searches: { put: [SEARCH] } });
  expect(await store.search("acc-2", SEARCH.id)).toBeNull();
  const read = await store.search("acc-1", SEARCH.id);
  read?.pages.push({ page: 1, ids: ["e2"] });
  expect(await store.searches("acc-1")).toEqual([SEARCH]);
  expect(await store.accounts()).toEqual(["acc-1"]);
  await store.commit("acc-1", { reset: ["searches"] });
  expect(await store.searches("acc-1")).toEqual([]);
  await store.commit("acc-1", { searches: { put: [SEARCH] } });
  await store.commit("acc-1", { searches: { remove: [SEARCH.id] } });
  expect(await store.search("acc-1", SEARCH.id)).toBeNull();
});
