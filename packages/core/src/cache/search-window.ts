// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { SEARCH_ROWS, WINDOW_SIZE } from "./limits";
import { syncMailboxes } from "./mailboxes";
import { land } from "./overlay";
import { resultRows } from "./rows";
import type { ListPage, ListRow } from "./rows";
import type { Scope } from "./scope";
import { scopeOf, searchFilterOf, searchParamsOf } from "./search";
import type { SearchParams } from "./search";
import type { MailStore, SearchRow } from "./store";
import { treePlaces } from "./tree-order";
import type { TreePlaces } from "./tree-order";
import { fetchPage, withPage } from "./window";
import type { Fetched } from "./window";

// One page of a search as a caller asks it: the key `searchKeyOf` wrote,
// the page, the locale the tree's names sort in and a signal that gives
// the request up. `again` runs the search anew on the first page alone.
export interface SearchAsk {
  key: string;
  page: number;
  locale: string;
  again?: boolean;
  signal?: AbortSignal;
}

// What a search's rows are read against: its scope and the tree's order.
interface View {
  scope: Scope;
  places: TreePlaces;
}

// The rows the held pages show now laid end to end, with the count of
// the threads the ids those pages hold stand for.
interface Shown {
  rows: ListRow[];
  threads: number;
}

// The held pages as the store's rows read them now: a thread with no
// member left in the scope shows no row and the rows after it close up.
// An id or a thread shows where it was held first, since the server
// shifts its rows as its results change.
async function shownOf(
  store: MailStore,
  accountId: string,
  row: SearchRow,
  { scope, places }: View,
): Promise<Shown> {
  const ids = [
    ...new Set(row.pages.toSorted((a, b) => a.page - b.page).flatMap((held) => held.ids)),
  ];
  const emails = await store.emails(accountId, ids);
  const threadIds = [...new Set([...emails.values()].map((email) => email.threadId))];
  const threads = await store.threads(accountId, threadIds);
  const page = {
    ids,
    total: row.total,
    pending: 0,
    emails: Object.fromEntries(emails),
    threads: Object.fromEntries(threads),
  };
  const threadsShown = new Set<string>();
  const rows = resultRows(page, scope, places).filter((shown) => {
    const fresh = !threadsShown.has(shown.threadId);
    threadsShown.add(shown.threadId);
    return fresh;
  });
  // An id the store holds no email for stands for a thread of its own.
  const gone = ids.filter((id) => !emails.has(id)).length;
  return { rows, threads: threadIds.length + gone };
}

function lastPage(row: SearchRow): SearchRow["pages"][number] | undefined {
  return row.pages.toSorted((a, b) => a.page - b.page).at(-1);
}

// A full last page says the server holds more; a short one ends the list.
function hasMore(row: SearchRow): boolean {
  return lastPage(row)?.ids.length === WINDOW_SIZE;
}

// The page as the user sees it: the rows from its place on, the total
// less the held threads that show no row, exact once the server has no more.
function pageFrom(row: SearchRow, shown: Shown, page: number): ListPage {
  const start = page * WINDOW_SIZE;
  const hidden = shown.threads - shown.rows.length;
  let total: number | null = shown.rows.length;
  if (hasMore(row)) {
    total = row.total === null ? null : Math.max(shown.rows.length, row.total - hidden);
  }
  return { rows: shown.rows.slice(start, start + WINDOW_SIZE), total, pending: 0 };
}

// A new search of the account and the held ones it pushes out: past
// SEARCH_ROWS the ones that ran first leave.
async function started(
  store: MailStore,
  accountId: string,
  key: string,
): Promise<{ row: SearchRow; leaving: string[] }> {
  const others = (await store.searches(accountId))
    .filter((held) => held.id !== key)
    .toSorted((a, b) => b.run - a.run);
  const run = (others.at(0)?.run ?? 0) + 1;
  return {
    row: { id: key, queryState: "", total: null, pages: [], run },
    leaving: others.slice(SEARCH_ROWS - 1).map((held) => held.id),
  };
}

// The account's mailboxes, fetched first while the store holds none.
async function mailboxesOf(client: JmapClient, store: MailStore): Promise<Mailbox[]> {
  if ((await store.state(client.accountId, "Mailbox")) === null) {
    await syncMailboxes(client, store);
  }
  return store.mailboxes(client.accountId);
}

// Where a search stands while it gathers the rows a page asks for.
interface Gathered {
  row: SearchRow;
  shown: Shown;
}

// What one ask reads on every page it gathers.
interface AskContext {
  client: JmapClient;
  store: MailStore;
  ask: SearchAsk;
  params: SearchParams;
  mailboxes: Mailbox[];
  view: View;
}

// The server's page `page` of the search, by anchor on `anchor` where
// one is given.
function serverPage(
  context: AskContext,
  page: number,
  anchor: string | undefined,
): Promise<Fetched> {
  const { client, store, ask, params, mailboxes } = context;
  return fetchPage(client, store, {
    filter: searchFilterOf(params, mailboxes),
    page,
    anchor,
    ...(ask.signal === undefined ? {} : { signal: ask.signal }),
  });
}

// The search with one more page, landed with the rows it brought.
async function landed(
  context: AskContext,
  base: { row: SearchRow; leaving: string[] },
  page: number,
  fetched: Fetched,
): Promise<Gathered> {
  const { client, store, view } = context;
  const row = withPage(base.row, page, fetched);
  await land(store, client.accountId, {
    emails: { put: fetched.emails },
    threads: { put: fetched.threads },
    searches: { put: [row], remove: base.leaving },
    states: fetched.states,
  });
  return { row, shown: await shownOf(store, client.accountId, row, view) };
}

// The search as the account holds it, else its first page from the server.
async function firstGathered(context: AskContext): Promise<Gathered> {
  const { client, store, ask, view } = context;
  const anew = ask.again === true && ask.page === 0;
  const row = anew ? null : await store.search(client.accountId, ask.key);
  if (row !== null) {
    return { row, shown: await shownOf(store, client.accountId, row, view) };
  }
  const fetched = await serverPage(context, 0, undefined);
  return landed(context, await started(store, client.accountId, ask.key), 0, fetched);
}

// The search with the server's pages after the held ones until its rows
// reach the page asked or the server has no more, each asked by anchor
// on the last row shown. A page that brings no id the search did not
// hold is kept and ends the gathering, since the next would bring the
// same.
async function gathered(context: AskContext, held: Gathered): Promise<Gathered> {
  const wanted = (context.ask.page + 1) * WINDOW_SIZE;
  if (held.shown.rows.length >= wanted || !hasMore(held.row)) {
    return held;
  }
  const page = (lastPage(held.row)?.page ?? -1) + 1;
  const fetched = await serverPage(context, page, held.shown.rows.at(-1)?.id);
  const known = new Set(held.row.pages.flatMap((kept) => kept.ids));
  const next = await landed(context, { row: held.row, leaving: [] }, page, fetched);
  return fetched.ids.some((id) => !known.has(id)) ? gathered(context, next) : next;
}

// One page of a search. The pages are a snapshot no poll refreshes, read
// against the store's rows as they stand, so a change the user makes
// shows; a page the rows that left leave short takes the server's next.
export async function searchWindow(
  client: JmapClient,
  store: MailStore,
  ask: SearchAsk,
): Promise<ListPage> {
  const mailboxes = await mailboxesOf(client, store);
  const params = searchParamsOf(ask.key);
  const view = { scope: scopeOf(params, mailboxes), places: treePlaces(mailboxes, ask.locale) };
  const context = { client, store, ask, params, mailboxes, view };
  const found = await gathered(context, await firstGathered(context));
  return pageFrom(found.row, found.shown, ask.page);
}
