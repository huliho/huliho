// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { SEARCH_ROWS } from "./limits";
import { syncMailboxes } from "./mailboxes";
import { land } from "./overlay";
import { resultRows } from "./rows";
import type { ListPage, ListRow } from "./rows";
import type { Scope } from "./scope";
import { scopeOf, searchFilterOf, searchParamsOf } from "./search";
import type { MailStore, SearchRow } from "./store";
import { treePlaces } from "./tree-order";
import type { TreePlaces } from "./tree-order";
import { fetchPage, pageOf, withPage } from "./window";

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

// The rows each held page shows now and how many held ids show none.
interface Shown {
  pages: Map<number, ListRow[]>;
  hidden: number;
}

// The held pages as the store's rows read them now: a thread with no
// member left in the scope shows no row. An id shows on the first page
// that holds it, since the server shifts its rows as its results change.
async function shownOf(
  store: MailStore,
  accountId: string,
  row: SearchRow,
  { scope, places }: View,
): Promise<Shown> {
  const ids = new Set(row.pages.flatMap((held) => held.ids));
  const emails = await store.emails(accountId, [...ids]);
  const threadIds = [...new Set([...emails.values()].map((email) => email.threadId))];
  const threads = await store.threads(accountId, threadIds);
  const base = {
    total: row.total,
    pending: 0,
    emails: Object.fromEntries(emails),
    threads: Object.fromEntries(threads),
  };
  const pages = new Map<number, ListRow[]>();
  const seen = new Set<string>();
  for (const held of row.pages.toSorted((a, b) => a.page - b.page)) {
    const fresh = held.ids.filter((id) => !seen.has(id));
    pages.set(held.page, resultRows({ ...base, ids: fresh }, scope, places));
    for (const id of fresh) {
      seen.add(id);
    }
  }
  const shown = [...pages.values()].reduce((sum, rows) => sum + rows.length, 0);
  return { pages, hidden: ids.size - shown };
}

function pageFrom(row: SearchRow, shown: Shown, page: number): ListPage {
  return {
    rows: shown.pages.get(page) ?? [],
    total: row.total === null ? null : Math.max(0, row.total - shown.hidden),
    pending: 0,
  };
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

// The search the account holds under the key with what its pages show
// now; null where it holds none or the first page is asked anew.
async function heldSearch(
  store: MailStore,
  accountId: string,
  ask: SearchAsk,
  view: View,
): Promise<{ row: SearchRow; shown: Shown } | null> {
  const anew = ask.again === true && ask.page === 0;
  const row = anew ? null : await store.search(accountId, ask.key);
  return row === null ? null : { row, shown: await shownOf(store, accountId, row, view) };
}

// One page of a search. The pages are a snapshot no poll refreshes, read
// against the store's rows as they stand, so a change the user makes shows.
export async function searchWindow(
  client: JmapClient,
  store: MailStore,
  ask: SearchAsk,
): Promise<ListPage> {
  const { accountId } = client;
  const mailboxes = await mailboxesOf(client, store);
  const params = searchParamsOf(ask.key);
  const view = { scope: scopeOf(params, mailboxes), places: treePlaces(mailboxes, ask.locale) };
  const held = await heldSearch(store, accountId, ask, view);
  if (held !== null && pageOf(held.row, ask.page) !== undefined) {
    return pageFrom(held.row, held.shown, ask.page);
  }
  const fetched = await fetchPage(client, store, {
    filter: searchFilterOf(params, mailboxes),
    page: ask.page,
    anchor: held?.shown.pages.get(ask.page - 1)?.at(-1)?.id,
    ...(ask.signal === undefined ? {} : { signal: ask.signal }),
  });
  const base =
    held === null ? await started(store, accountId, ask.key) : { row: held.row, leaving: [] };
  const row = withPage(base.row, ask.page, fetched);
  await land(store, accountId, {
    emails: { put: fetched.emails },
    threads: { put: fetched.threads },
    searches: { put: [row], remove: base.leaving },
    states: fetched.states,
  });
  return pageFrom(row, await shownOf(store, accountId, row, view), ask.page);
}
