// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient, MethodFailure } from "../jmap/client";
import type { EmailHeader } from "../jmap/schemas";
import { ACCOUNT, FakeJmap, at, email, mailbox } from "./fake-jmap";
import { SEARCH_ROWS, WINDOW_SIZE } from "./limits";
import { MemoryMailStore } from "./memory";
import { dropHeld, mutateMany } from "./pending";
import type { PatchMutation } from "./pending";
import { applyChanges } from "./poll";
import type { ListPage } from "./rows";
import { searchKeyOf } from "./search";
import type { SearchParams } from "./search";
import { searchWindow } from "./search-window";
import type { SearchAsk } from "./search-window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

function titled(id: string, facts: Parameters<typeof email>[1]): EmailHeader {
  return { ...email(id, facts), subject: `Invoice ${id}` };
}

function move(id: string, from: string, to: string): PatchMutation {
  return { type: "Email", id, patch: { [`mailboxIds/${from}`]: null, [`mailboxIds/${to}`]: true } };
}

// The thread t1 with e1 in the archive and e2 in the inbox, e3 in the
// inbox, e4 in Junk, e5 in Trash and e6 in a folder all name an invoice;
// e7 in the inbox does not.
function serve(): Rig {
  const server = new FakeJmap();
  for (const [id, role] of [
    ["inbox", "inbox"],
    ["archive", "archive"],
    ["junk", "junk"],
    ["trash", "trash"],
    ["work", null],
  ] as const) {
    server.putMailbox(mailbox(id, role));
  }
  server.addEmail(titled("e1", { threadId: "t1", mailboxIds: ["archive"], receivedAt: at(1) }));
  server.addEmail(titled("e2", { threadId: "t1", receivedAt: at(2) }));
  server.addEmail(titled("e3", { receivedAt: at(3) }));
  server.addEmail(titled("e4", { mailboxIds: ["junk"], receivedAt: at(4) }));
  server.addEmail(titled("e5", { mailboxIds: ["trash"], receivedAt: at(5) }));
  server.addEmail(titled("e6", { mailboxIds: ["work"], receivedAt: at(6) }));
  server.addEmail(email("e7", { receivedAt: at(7) }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

function ask(params: SearchParams, page = 0): SearchAsk {
  return { key: searchKeyOf(params), page, locale: "en" };
}

// The rows of a page as their ids and the mailboxes they act from.
function rowsOf(page: ListPage): [string, string][] {
  return page.rows.map((row) => [row.id, row.mailboxId]);
}

// The arguments of every Email/query sent so far.
function queries(server: FakeJmap): Record<string, unknown>[] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/query")
    .map(([, args]) => args);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a search fetches the tree first, then sends one query that leaves Junk and Trash out (RFC 8621 section 4.4.1)", async () => {
  const { server, client, store } = serve();
  const page = await searchWindow(client, store, ask({ q: "invoice" }));
  expect(server.posted()[0]?.[0]?.[0]).toBe("Mailbox/get");
  expect(queries(server)).toEqual([
    {
      accountId: "u1",
      filter: { inMailboxOtherThan: ["junk", "trash"], text: "invoice" },
      sort: [{ property: "receivedAt", isAscending: false }],
      position: 0,
      limit: WINDOW_SIZE,
      calculateTotal: true,
      collapseThreads: true,
    },
  ]);
  expect(rowsOf(page)).toEqual([
    ["e6", "work"],
    ["e3", "inbox"],
    ["e2", "inbox"],
  ]);
  expect(page.total).toBe(3);
  expect(page.rows[2]).toMatchObject({ threadId: "t1", memberIds: ["e2"], count: 1 });
});

test("a held page answers from the store without a request, and `again` runs the search anew from its first page", async () => {
  const { server, client, store } = serve();
  await searchWindow(client, store, ask({ q: "invoice" }));
  server.requests.length = 0;
  expect(rowsOf(await searchWindow(client, store, ask({ q: "invoice" })))).toHaveLength(3);
  expect(server.requests).toHaveLength(0);
  server.addEmail(titled("e8", { receivedAt: at(8) }));
  const again = await searchWindow(client, store, { ...ask({ q: "invoice" }), again: true });
  expect(queries(server)).toHaveLength(1);
  expect(again.rows[0]?.id).toBe("e8");
  expect(again.total).toBe(4);
  // On a later page the flag changes nothing, and a first page short of a
  // full one says the server holds no more, so nothing is asked.
  const later = await searchWindow(client, store, { ...ask({ q: "invoice" }, 1), again: true });
  expect(queries(server)).toHaveLength(1);
  expect(later).toEqual({ rows: [], total: 4, pending: 0 });
  const held = await store.search(ACCOUNT, searchKeyOf({ q: "invoice" }));
  expect(held?.pages.map((page) => [page.page, page.ids.length])).toEqual([[0, 4]]);
});

test("`all` keeps Junk and Trash in, and one mailbox narrows the search to it", async () => {
  const { server, client, store } = serve();
  const all = await searchWindow(client, store, ask({ q: "invoice", in: "all" }));
  expect(rowsOf(all).map(([id]) => id)).toEqual(["e6", "e5", "e4", "e3", "e2"]);
  expect(rowsOf(all)).toContainEqual(["e5", "trash"]);
  const junk = await searchWindow(client, store, ask({ q: "invoice", in: "junk" }));
  expect(rowsOf(junk)).toEqual([["e4", "junk"]]);
  expect(queries(server).map((args) => args["filter"])).toEqual([
    { text: "invoice" },
    { inMailbox: "junk", text: "invoice" },
  ]);
});

test("under the default scope an archive keeps the row with its mailbox renewed, a delete takes it out with the total and a flip shows in place", async () => {
  const { client, store } = serve();
  const search = ask({ q: "invoice" });
  await searchWindow(client, store, search);
  await mutateMany(store, ACCOUNT, [move("e3", "inbox", "archive")]);
  await mutateMany(store, ACCOUNT, [move("e6", "work", "trash")]);
  await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e2", patch: { "keywords/$seen": true } },
  ]);
  const page = await searchWindow(client, store, search);
  expect(rowsOf(page)).toEqual([
    ["e3", "archive"],
    ["e2", "inbox"],
  ]);
  expect(page.total).toBe(2);
  expect(page.rows[1]?.unread).toBe(false);
});

test("a thread stays while any member stays in scope and acts from where that member stands", async () => {
  const { client, store } = serve();
  const search = ask({ q: "invoice" });
  await searchWindow(client, store, search);
  await mutateMany(store, ACCOUNT, [move("e2", "inbox", "trash")]);
  const page = await searchWindow(client, store, search);
  expect(page.rows.find((row) => row.threadId === "t1")).toMatchObject({
    id: "e2",
    mailboxId: "archive",
    memberIds: ["e1"],
  });
  expect(page.total).toBe(3);
});

test("under a mailbox scope an archive takes the row out, and its undo puts it back at its place", async () => {
  const { client, store } = serve();
  const search = ask({ q: "invoice", in: "inbox" });
  expect(rowsOf(await searchWindow(client, store, search))).toEqual([
    ["e3", "inbox"],
    ["e2", "inbox"],
  ]);
  const logged = await mutateMany(store, ACCOUNT, [move("e3", "inbox", "archive")]);
  const archived = await searchWindow(client, store, search);
  expect(rowsOf(archived)).toEqual([["e2", "inbox"]]);
  expect(archived.total).toBe(1);
  await mutateMany(store, ACCOUNT, logged.inverses);
  const back = await searchWindow(client, store, search);
  expect(rowsOf(back)).toEqual([
    ["e3", "inbox"],
    ["e2", "inbox"],
  ]);
  expect(back.total).toBe(2);
});

test("under `all` a delete keeps the row in Trash, while a held destroy hides it until it is dropped", async () => {
  const { client, store } = serve();
  const search = ask({ q: "invoice", in: "all" });
  await searchWindow(client, store, search);
  await mutateMany(store, ACCOUNT, [move("e3", "inbox", "trash")]);
  expect(rowsOf(await searchWindow(client, store, search))).toContainEqual(["e3", "trash"]);
  const logged = await mutateMany(store, ACCOUNT, [{ type: "Email", id: "e5", destroy: true }]);
  const hidden = await searchWindow(client, store, search);
  expect(rowsOf(hidden).map(([id]) => id)).toEqual(["e6", "e4", "e3", "e2"]);
  expect(hidden.total).toBe(4);
  await dropHeld(store, ACCOUNT, logged.held);
  expect(rowsOf(await searchWindow(client, store, search)).map(([id]) => id)).toEqual([
    "e6",
    "e5",
    "e4",
    "e3",
    "e2",
  ]);
});

test("a poll never adds a row to the results, while an email the server destroyed leaves them", async () => {
  const { server, client, store } = serve();
  const search = ask({ q: "invoice" });
  await searchWindow(client, store, search);
  server.addEmail(titled("e8", { receivedAt: at(8) }));
  server.destroyEmail("e6");
  server.recount();
  await applyChanges(client, store, []);
  server.requests.length = 0;
  const page = await searchWindow(client, store, search);
  expect(server.requests).toHaveLength(0);
  expect(rowsOf(page).map(([id]) => id)).toEqual(["e3", "e2"]);
  expect(page.total).toBe(2);
});

// The mailings in the inbox past one page of results, b0 the oldest.
const BULK = WINDOW_SIZE + 2;
const HALF_SECOND_MS = 500;

function mailing(id: string, receivedAt: string, threadId?: string): EmailHeader {
  const facts = threadId === undefined ? { receivedAt } : { receivedAt, threadId };
  return { ...email(id, facts), subject: "Bulk" };
}

// `count` mailings a second apart, b0 the oldest.
function bulk(server: FakeJmap, count = BULK): void {
  for (let index = 0; index < count; index += 1) {
    server.addEmail(mailing(`b${String(index)}`, at(count + index)));
  }
}

test("a row that leaves a full page lets the rows after it close up, the server's next page asked by anchor on the last row shown (RFC 8620 section 5.5)", async () => {
  const { server, client, store } = serve();
  bulk(server);
  const search = ask({ q: "bulk", in: "inbox" });
  const first = await searchWindow(client, store, search);
  const last = first.rows.at(-1)?.id ?? "";
  const before = first.rows.at(-2)?.id ?? "";
  await mutateMany(store, ACCOUNT, [move(last, "inbox", "archive")]);
  const again = await searchWindow(client, store, search);
  expect(queries(server).at(-1)).toMatchObject({ anchor: before, anchorOffset: 1 });
  expect(again.rows).toHaveLength(WINDOW_SIZE);
  expect(again.rows.at(-2)?.id).toBe(before);
  expect(again.rows.at(-1)?.id).toBe("b1");
  expect(again.total).toBe(BULK - 1);
  const second = await searchWindow(client, store, { ...search, page: 1 });
  expect(second.rows.map((row) => row.id)).toEqual(["b0"]);
  expect(second.total).toBe(BULK - 1);
});

test("a later page asked first brings the pages before it, each by anchor on the one before", async () => {
  const { server, client, store } = serve();
  bulk(server);
  const second = await searchWindow(client, store, ask({ q: "bulk", in: "inbox" }, 1));
  expect(queries(server).map((args) => [args["position"], args["anchor"]])).toEqual([
    [0, undefined],
    [undefined, "b2"],
  ]);
  expect(second.rows.map((row) => row.id)).toEqual(["b1", "b0"]);
  expect(second.total).toBe(BULK);
});

test("a page that brings only rows the search holds is kept and ends the gathering with the total it leaves", async () => {
  const { server, client, store } = serve();
  bulk(server);
  const search = ask({ q: "bulk", in: "inbox" });
  await searchWindow(client, store, search);
  server.destroyEmail("b1");
  server.destroyEmail("b0");
  await mutateMany(store, ACCOUNT, [
    move("b4", "inbox", "archive"),
    move("b3", "inbox", "archive"),
    move("b2", "inbox", "archive"),
  ]);
  const page = await searchWindow(client, store, search);
  expect(queries(server)).toHaveLength(2);
  expect(page.rows).toHaveLength(WINDOW_SIZE - 3);
  expect(page.total).toBe(WINDOW_SIZE - 3);
});

test("a row the server shifts onto the next page shows once, on the page that held it first", async () => {
  const { server, client, store } = serve();
  bulk(server);
  const search = ask({ q: "bulk", in: "inbox" });
  const first = await searchWindow(client, store, search);
  // The anchor leaves the server and two newer mailings push the rest down.
  server.destroyEmail(first.rows.at(-1)?.id ?? "");
  server.addEmail(mailing("n1", at(3 * BULK)));
  server.addEmail(mailing("n2", at(3 * BULK + 1)));
  const second = await searchWindow(client, store, { ...search, page: 1 });
  expect(second.rows.map((row) => row.id)).toEqual(["b1", "b0"]);
  expect(second.total).toBe(BULK);
});

test("a thread the server answers again under another member counts once in the total while more pages remain", async () => {
  const { server, client, store } = serve();
  const count = 3 * WINDOW_SIZE + 2;
  bulk(server, count);
  // The thread tx: x1 the newest mailing of all, x2 half a second past
  // one the second page holds.
  server.addEmail(mailing("x1", at(3 * count), "tx"));
  server.addEmail(mailing("x2", at(count + WINDOW_SIZE + WINDOW_SIZE / 2, HALF_SECOND_MS), "tx"));
  const search = ask({ q: "bulk", in: "inbox" });
  const first = await searchWindow(client, store, search);
  expect(first.rows[0]?.id).toBe("x1");
  // Another client takes x1 away, so the server answers the thread under x2 from now on.
  server.destroyEmail("x1");
  const second = await searchWindow(client, store, { ...search, page: 1 });
  expect(second.rows).toHaveLength(WINDOW_SIZE);
  expect(second.rows.map((row) => row.id)).not.toContain("x2");
  expect(second.total).toBe(count + 1);
});

test("past SEARCH_ROWS searches the one that ran first leaves, and asking it again searches anew", async () => {
  const { server, client, store } = serve();
  const searches = Array.from({ length: SEARCH_ROWS + 1 }, (_, index) =>
    ask({ q: `invoice e${String(index + 1)}` }),
  );
  // One after the other, since the order they run in decides which leaves.
  await searches.reduce(async (previous, search) => {
    await previous;
    await searchWindow(client, store, search);
  }, Promise.resolve());
  const held = await store.searches(ACCOUNT);
  expect(held.map((row) => row.id).toSorted()).toEqual(
    searches
      .slice(1)
      .map((search) => search.key)
      .toSorted(),
  );
  const sent = queries(server).length;
  const [first] = searches;
  if (first !== undefined) {
    await searchWindow(client, store, first);
  }
  expect(queries(server)).toHaveLength(sent + 1);
});

test("a search its caller gives up rejects as fetch does, lands nothing and holds no row", async () => {
  const { server, client, store } = serve();
  await searchWindow(client, store, ask({ q: "nothing" }));
  server.holdSearches = true;
  const controller = new AbortController();
  const asked = searchWindow(client, store, {
    ...ask({ q: "invoice" }),
    signal: controller.signal,
  });
  await vi.waitFor(() => {
    expect(server.waiting).toBe(1);
  });
  controller.abort();
  await expect(asked).rejects.toMatchObject({ name: "AbortError" });
  expect((await store.searches(ACCOUNT)).map((row) => row.id)).toEqual([
    searchKeyOf({ q: "nothing" }),
  ]);
  expect((await store.emails(ACCOUNT, ["e3"])).size).toBe(0);
});

test("a server that refuses a search fails the call with its error and lands nothing", async () => {
  const { server, client, store } = serve();
  server.searchError = "serverUnavailable";
  const failed = searchWindow(client, store, ask({ q: "invoice" }));
  await expect(failed).rejects.toBeInstanceOf(MethodFailure);
  await expect(failed).rejects.toMatchObject({ type: "serverUnavailable" });
  server.searchError = "unsupportedFilter";
  await expect(searchWindow(client, store, ask({ from: "sanne" }))).rejects.toMatchObject({
    type: "unsupportedFilter",
  });
  expect(await store.searches(ACCOUNT)).toEqual([]);
});

test("a state past the server's horizon drops the held searches with the rows they read (RFC 8620 section 5.2)", async () => {
  const { server, client, store } = serve();
  await searchWindow(client, store, ask({ q: "invoice" }));
  server.addEmail(titled("e8", { receivedAt: at(8) }));
  server.forget();
  await applyChanges(client, store, []);
  expect(await store.searches(ACCOUNT)).toEqual([]);
  expect((await store.emails(ACCOUNT, ["e3"])).size).toBe(0);
});
