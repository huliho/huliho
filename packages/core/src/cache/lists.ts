// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MemberChange } from "./counts";
import type { MailStore, MemberState, QueryRow } from "./store";

type Members = Record<string, MemberState>;

// One email whose mailboxes may change, with where it sorts in a list
// and whether a thread it brings into a mailbox takes its place there
// again, as an undo or a refusal does.
export interface ListChange extends MemberChange {
  receivedAt: string;
  restores: boolean;
}

// Takes `ids` out of every page of the list, the marker's count and the
// fresh page; the total drops by the rows that left a page. True when a
// row left a page.
export function strip(row: QueryRow, ids: ReadonlySet<string>): boolean {
  let left = false;
  for (const page of row.pages) {
    const kept = page.ids.filter((id) => !ids.has(id));
    if (kept.length !== page.ids.length) {
      row.total =
        row.total === null ? null : Math.max(0, row.total - page.ids.length + kept.length);
      page.ids = kept;
      left = true;
    }
  }
  row.pending = row.pending.filter((id) => !ids.has(id));
  if (row.fresh !== null) {
    row.fresh.ids = row.fresh.ids.filter((id) => !ids.has(id));
  }
  return left;
}

function standsIn(members: Members, mailboxId: string): boolean {
  return Object.values(members).some((member) => mailboxId in member.mailboxIds);
}

// A thread that left the list's mailbox leaves its pages, the marker's
// count and the fresh page; a total drops with each row that left.
function leave(row: QueryRow, ids: ReadonlySet<string>): boolean {
  const fresh = row.fresh?.ids.length ?? 0;
  const left = strip(row, ids);
  if (row.fresh !== null && row.fresh.total !== null && row.fresh.ids.length < fresh) {
    row.fresh.total = Math.max(0, row.fresh.total - 1);
  }
  return left;
}

// The thread's exemplar back at its place by receivedAt, newest first:
// before the first held row older than it, after the last one when the
// list holds every row, else left for the page that holds its place.
// The total grows with the row that came back.
function enter(row: QueryRow, exemplar: ListChange, times: ReadonlyMap<string, number>): boolean {
  const time = Date.parse(exemplar.receivedAt);
  const older = (id: string): boolean => (times.get(id) ?? Number.POSITIVE_INFINITY) < time;
  const pages = row.pages.toSorted((a, b) => a.page - b.page);
  const loaded = pages.reduce((sum, page) => sum + page.ids.length, 0);
  const page = pages.find((held) => held.ids.some(older));
  const last = pages.at(-1);
  if (page !== undefined) {
    page.ids.splice(page.ids.findIndex(older), 0, exemplar.id);
  } else if (last !== undefined && row.total !== null && loaded >= row.total) {
    last.ids.push(exemplar.id);
  } else {
    return false;
  }
  row.total = row.total === null ? null : row.total + 1;
  return true;
}

// How one thread stands in the lists before and after a change.
interface ThreadMove {
  ids: Set<string>;
  before: Members;
  after: Members;
  changes: ListChange[];
}

function threadMovesOf(
  changes: readonly ListChange[],
  members: ReadonlyMap<string, Members>,
): ThreadMove[] {
  const threadIds = new Set(changes.map((change) => change.threadId));
  return [...threadIds].map((threadId) => {
    const inThread = changes.filter((change) => change.threadId === threadId);
    const base = members.get(threadId) ?? {};
    const side = (pick: (change: ListChange) => MemberState): Members => ({
      ...base,
      ...Object.fromEntries(inThread.map((change) => [change.id, pick(change)])),
    });
    return {
      ids: new Set([...Object.keys(base), ...inThread.map((change) => change.id)]),
      before: side((change) => change.before),
      after: side((change) => change.after),
      changes: inThread,
    };
  });
}

// The newest email a restoring change brings into the mailbox, which the
// list shows for its thread; none for a change that moves forward.
function returning(move: ThreadMove, mailboxId: string): ListChange | undefined {
  return move.changes
    .filter(
      (change) =>
        change.restores &&
        mailboxId in change.after.mailboxIds &&
        !(mailboxId in change.before.mailboxIds),
    )
    .reduce<ListChange | undefined>(
      (newest, change) =>
        newest === undefined || Date.parse(change.receivedAt) > Date.parse(newest.receivedAt)
          ? change
          : newest,
      undefined,
    );
}

// What one thread's move does to one list: true when the list changed.
function moveIn(row: QueryRow, move: ThreadMove, times: ReadonlyMap<string, number>): boolean {
  const was = standsIn(move.before, row.id);
  const is = standsIn(move.after, row.id);
  if (was && !is) {
    return leave(row, move.ids);
  }
  const exemplar = was || !is ? undefined : returning(move, row.id);
  if (exemplar === undefined || row.pages.some((page) => page.ids.some((id) => move.ids.has(id)))) {
    return false;
  }
  return enter(row, exemplar, times);
}

// The receipt times of every row the lists hold, read only when a
// thread comes back to one of them.
async function timesOf(
  store: MailStore,
  accountId: string,
  rows: readonly QueryRow[],
): Promise<Map<string, number>> {
  const ids = rows.flatMap((row) => row.pages.flatMap((page) => page.ids));
  const held = await store.emails(accountId, ids);
  return new Map([...held.values()].map((header) => [header.id, Date.parse(header.receivedAt)]));
}

// The lists after emails changed mailboxes. A thread whose members in a
// mailbox all left it leaves every page of that list at once; a thread
// that entered a mailbox stays out of its list until the next fetch,
// unless the change restores it, which puts it back at its place.
// `members` holds what each thread knew before. Answers the lists that
// changed.
export async function listsAfter(
  store: MailStore,
  accountId: string,
  changes: readonly ListChange[],
  members: ReadonlyMap<string, Members>,
): Promise<QueryRow[]> {
  const moves = threadMovesOf(changes, members);
  const rows = await store.queries(accountId);
  const restoring = changes.some((change) => change.restores);
  const times = restoring ? await timesOf(store, accountId, rows) : new Map<string, number>();
  const changed: QueryRow[] = [];
  for (const row of rows) {
    if (moves.map((move) => moveIn(row, move, times)).includes(true)) {
      changed.push(row);
    }
  }
  return changed;
}
