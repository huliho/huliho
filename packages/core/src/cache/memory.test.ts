// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { at, email, mailbox } from "./fake-jmap";
import { MemoryMailStore } from "./memory";
import type { QueryRow } from "./store";

const ROW: QueryRow = {
  id: "inbox",
  queryState: "3",
  total: 1,
  pages: [{ page: 0, ids: ["e1"] }],
  pending: [],
  fresh: null,
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
