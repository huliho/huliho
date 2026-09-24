// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import { WINDOW_SIZE } from "./limits";
import type { MailStore, QueryRow } from "./store";
import { fetchAfter, fetchFirstPage, nothingFetched, pageOf } from "./window";
import type { Fetched, Filler } from "./window";

// What the list showed when the refresh ran: the ids of every held
// page, the thread each stands for and the receipt time of the oldest.
interface Shown {
  ids: Set<string>;
  threadOf: Map<string, string>;
  oldest: number | null;
}

// One refresh of a list: the fresh first page, what the list showed,
// the new mail that waits and the threads that mail stands for.
interface Refresh {
  row: QueryRow;
  fetched: Fetched;
  shown: Shown;
  pending: string[];
  pendingThreads: Set<string>;
}

async function shownOf(store: MailStore, accountId: string, row: QueryRow): Promise<Shown> {
  const held = row.pages.flatMap((page) => page.ids);
  const rows = [...(await store.emails(accountId, held)).values()];
  const times = rows.map((email) => Date.parse(email.receivedAt));
  return {
    ids: new Set(held),
    threadOf: new Map(rows.map((email) => [email.id, email.threadId])),
    oldest: times.length === 0 ? null : Math.min(...times),
  };
}

// The fresh ids the list does not hold whose mail is newer than the
// oldest row it showed; none when it showed none, so nothing counts as
// newer than what was there.
function pendingOf(shown: Shown, fetched: Fetched): string[] {
  const arrived = new Map(fetched.emails.map((email) => [email.id, Date.parse(email.receivedAt)]));
  return fetched.ids.filter(
    (id) =>
      !shown.ids.has(id) && shown.oldest !== null && (arrived.get(id) ?? Number.NaN) > shown.oldest,
  );
}

function refreshOf(row: QueryRow, fetched: Fetched, shown: Shown): Refresh {
  const pending = pendingOf(shown, fetched);
  const waiting = new Set(pending);
  const pendingThreads = new Set(
    fetched.emails.filter((email) => waiting.has(email.id)).map((email) => email.threadId),
  );
  return { row, fetched, shown, pending, pendingThreads };
}

// The first page as the server orders it now. Without new mail the
// fresh page is the list. New mail, the exemplars newer than the oldest
// row the list showed, waits in `pending` and `fresh` for the user; the
// rows the user saw stay where they are and the first page takes the
// rows below them, since a row below the ones shown moves nothing
// under the user.
export async function refreshFirstPage(
  client: JmapClient,
  store: MailStore,
  row: QueryRow,
): Promise<void> {
  const fetched = await fetchFirstPage(client, store, row.id);
  const refresh = refreshOf(row, fetched, await shownOf(store, client.accountId, row));
  const filler = await topUp(client, store, refresh);
  await store.commit(client.accountId, {
    emails: { put: [...fetched.emails, ...filler.emails] },
    threads: { put: [...fetched.threads, ...filler.threads] },
    queries: { put: [landed(refresh, filler.ids)] },
    states: fetched.states,
  });
}

// The rows the list shows before the reveal: the server's total less
// the new mail whose thread has no row in the list yet; a reply to a
// shown thread takes that row's place when revealed.
function totalOf({ row, fetched, shown, pendingThreads }: Refresh): number | null {
  if (fetched.total === null) {
    return row.total;
  }
  const shownThreads = new Set(shown.threadOf.values());
  const added = [...pendingThreads].filter((thread) => !shownThreads.has(thread));
  return fetched.total - added.length;
}

// Whether the server still lists a held row. One it does not list is
// either pushed past the fresh page by new mail or replaced by its
// thread's newer mail, which waits.
function stillListed(refresh: Refresh, listed: ReadonlySet<string>, id: string): boolean {
  const thread = refresh.shown.threadOf.get(id);
  return listed.has(id) || thread === undefined || !refresh.pendingThreads.has(thread);
}

// The rows that top up a first page held short of a full one while new
// mail waits and rows follow it: the ones after its last row the server
// still lists, from the fresh page where it reaches them and by anchor
// on the fresh page's end for the rest. A fresh page short of a full
// one is the whole list, so nothing follows it.
async function topUp(client: JmapClient, store: MailStore, refresh: Refresh): Promise<Filler> {
  const { row, fetched, pending } = refresh;
  if (pending.length === 0) {
    return nothingFetched();
  }
  const first = pageOf(row, 0) ?? [];
  const room = Math.max(0, Math.min(WINDOW_SIZE, totalOf(refresh) ?? WINDOW_SIZE) - first.length);
  const listed = new Set(fetched.ids);
  const anchor = first.findLast((id) => stillListed(refresh, listed, id));
  const at = anchor === undefined ? -1 : fetched.ids.indexOf(anchor);
  const waiting = new Set(pending);
  const reached =
    at < 0
      ? []
      : fetched.ids
          .slice(at + 1)
          .filter((id) => !waiting.has(id))
          .slice(0, room);
  const rest = await fetchAfter(client, store, {
    mailboxId: row.id,
    after: at < 0 ? anchor : fetched.ids.at(-1),
    wanted: fetched.ids.length < WINDOW_SIZE ? 0 : room - reached.length,
  });
  return { ...rest, ids: [...reached, ...rest.ids] };
}

// The row after a refresh. Without new mail the fresh page is the list.
// With it, the rows the user saw stay where they are, the first page
// grows by the rows that top it up and the new mail waits in `fresh`
// until revealed.
function landed(refresh: Refresh, topped: readonly string[]): QueryRow {
  const { row, fetched, pending } = refresh;
  const first = pageOf(row, 0) ?? [];
  if (pending.length === 0) {
    return {
      ...row,
      queryState: fetched.queryState,
      total: fetched.total,
      pages: sameIds(first, fetched.ids) ? row.pages : [{ page: 0, ids: fetched.ids }],
      pending: [],
      fresh: null,
    };
  }
  return {
    ...row,
    total: totalOf(refresh),
    // The deeper pages were anchored on the first page's old end.
    pages: topped.length === 0 ? row.pages : [{ page: 0, ids: [...first, ...topped] }],
    pending: [...pending],
    fresh: { ids: fetched.ids, total: fetched.total, queryState: fetched.queryState },
  };
}

function sameIds(first: readonly string[], second: readonly string[]): boolean {
  return first.length === second.length && first.every((id, index) => id === second.at(index));
}

// The user brings the new mail in: the fresh first page becomes the
// list and every deeper page is fetched anew from it.
export async function revealNewMail(
  store: MailStore,
  accountId: string,
  mailboxId: string,
): Promise<void> {
  const row = await store.query(accountId, mailboxId);
  if (row === null || row.fresh === null) {
    return;
  }
  const { fresh } = row;
  await store.commit(accountId, {
    queries: {
      put: [
        {
          ...row,
          queryState: fresh.queryState,
          total: fresh.total,
          pages: [{ page: 0, ids: fresh.ids }],
          pending: [],
          fresh: null,
        },
      ],
    },
  });
}
