// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import { movesOf, recounted, shiftOf } from "./counts";
import type { MemberChange, Moves } from "./counts";
import { strip } from "./lists";
import { memberOf } from "./members";
import { replayed, unpatched } from "./patch";
import type { Batch, MailStore, MemberState, PendingRow, QueryRow, ThreadRow } from "./store";

type Pending = Map<string, PendingRow[]>;

// Where rows come from. A round of the change logs brings every mailbox
// up to date within its poll; a fetch of a list or a thread brings none.
type LandingKind = "round" | "fetch";

// The rows a landing writes that the counts and the lists read.
interface Landed {
  headers: EmailHeader[];
  threads: ThreadRow[];
  mailboxes: readonly Mailbox[];
}

// The mailbox rows a landing writes: its own and the stored ones a
// refit shifted.
interface Recount {
  fresh: Mailbox[];
  shifted: Mailbox[];
}

// The header of every pending email and the members of its thread, as
// the landing or the store holds them.
interface Standing {
  headers: Map<string, EmailHeader>;
  threads: Map<string, ThreadRow>;
}

function byEmail(rows: readonly PendingRow[]): Pending {
  const grouped: Pending = new Map();
  for (const row of rows) {
    grouped.set(row.id, [...(grouped.get(row.id) ?? []), row]);
  }
  return grouped;
}

// The rows whose inverse a replay changed.
function refitted(before: readonly PendingRow[], after: readonly PendingRow[]): PendingRow[] {
  const held = new Map(before.map((row) => [row.seq, JSON.stringify(row.inverse)]));
  return after.filter((row) => held.get(row.seq) !== JSON.stringify(row.inverse));
}

// The headers that land with the pending changes of their emails laid
// over them. A row's inverse follows the header it now stands on, so
// taking the change back restores what the server said last.
function relaidHeaders(
  headers: readonly EmailHeader[],
  pending: Pending,
): { headers: EmailHeader[]; fitted: PendingRow[] } {
  const fitted: PendingRow[] = [];
  const relaid = headers.map((header) => {
    const rows = pending.get(header.id);
    if (rows === undefined) {
      return header;
    }
    const replay = replayed(memberOf(header), rows);
    fitted.push(...refitted(rows, replay.rows));
    return { ...header, ...replay.state };
  });
  return { headers: relaid, fitted };
}

// The threads that land with the pending changes of their members laid
// over them. An email no header stands for anywhere takes the inverse
// that fits the member that landed, as a header refits it.
function relaidThreads(
  threads: readonly ThreadRow[],
  pending: Pending,
  headed: ReadonlySet<string>,
): { threads: ThreadRow[]; fitted: PendingRow[] } {
  const fitted: PendingRow[] = [];
  const relaid = threads.map((thread) => ({
    ...thread,
    members: Object.fromEntries(
      Object.entries(thread.members).map(([id, member]) => {
        const rows = pending.get(id);
        if (rows === undefined) {
          return [id, member];
        }
        const replay = replayed(member, rows);
        if (!headed.has(id)) {
          fitted.push(...refitted(rows, replay.rows));
        }
        return [id, replay.state];
      }),
    ),
  }));
  return { threads: relaid, fitted };
}

// The rows a landing holds or the store holds, the landing first.
async function known<Row extends { id: string }>(
  landing: readonly Row[],
  ids: readonly string[],
  read: (ids: readonly string[]) => Promise<Map<string, Row>>,
): Promise<Map<string, Row>> {
  const landed = new Map(landing.map((row) => [row.id, row]));
  const stored = await read(ids.filter((id) => !landed.has(id)));
  return new Map([...stored, ...landed]);
}

// The threads of the pending emails: their headers' and, for an email
// no header stands for, the one its rows name.
async function standingOf(
  store: MailStore,
  accountId: string,
  landing: Landed,
  { headers, pending }: { headers: Map<string, EmailHeader>; pending: Pending },
): Promise<Standing> {
  const named = [...pending.values()].flat().flatMap((row) => row.threadId ?? []);
  const threadIds = [
    ...new Set([...[...headers.values()].map((header) => header.threadId), ...named]),
  ];
  const threads = await known(landing.threads, threadIds, (ids) => store.threads(accountId, ids));
  return { headers, threads };
}

// Where a pending email stands now: its header's thread and state, else
// the member its rows' thread holds.
function placeOf(
  { headers, threads }: Standing,
  id: string,
  rows: readonly PendingRow[],
): { threadId: string; state: MemberState } | null {
  const header = headers.get(id);
  if (header !== undefined) {
    return { threadId: header.threadId, state: memberOf(header) };
  }
  const threadId = rows.find((row) => row.threadId !== undefined)?.threadId;
  const thread = threadId === undefined ? undefined : threads.get(threadId);
  const state = thread === undefined ? undefined : new Map(Object.entries(thread.members)).get(id);
  return threadId === undefined || state === undefined ? null : { threadId, state };
}

// How far the pending rows move the counts away from what the server
// holds, which their inverses say.
function pendingMoves(standing: Standing, pending: Pending): Moves {
  const changes = [...pending].flatMap(([id, rows]): MemberChange[] => {
    const place = placeOf(standing, id, rows);
    if (place === null) {
      return [];
    }
    const after = place.state;
    return [{ id, threadId: place.threadId, before: unpatched(after, rows), after }];
  });
  const members = new Map([...standing.threads].map(([id, thread]) => [id, thread.members]));
  return movesOf(members, changes);
}

// The mailbox rows a landing writes. A row that lands carries the
// server's count, so the pending moves are counted in again; in a round
// a row that does not land takes the difference a refit makes.
async function relaidMailboxes(
  store: MailStore,
  accountId: string,
  { landing, standing }: { landing: Landed; standing: Standing },
  logs: { held: Pending; pending: Pending; shifts: boolean },
): Promise<Recount> {
  if (landing.mailboxes.length === 0 && !logs.shifts) {
    return { fresh: [], shifted: [] };
  }
  const moved = pendingMoves(standing, logs.pending);
  const fresh = landing.mailboxes.map((row) => recounted([row], moved).at(0) ?? row);
  if (!logs.shifts) {
    return { fresh, shifted: [] };
  }
  const landed = new Set(landing.mailboxes.map((row) => row.id));
  const stored = (await store.mailboxes(accountId)).filter((row) => !landed.has(row.id));
  return { fresh, shifted: recounted(stored, shiftOf(pendingMoves(standing, logs.held), moved)) };
}

// The lists that land without the threads a pending change took out of
// their mailbox: an archived thread stays archived whatever a poll
// fetched, and a destroy waiting in the log hides its email.
function relaidLists(
  queries: readonly QueryRow[],
  standing: Standing,
  pending: Pending,
): QueryRow[] {
  const gone = [...pending].flatMap(([id, rows]) => {
    const place = placeOf(standing, id, rows);
    if (place === null) {
      return [];
    }
    const thread = standing.threads.get(place.threadId);
    const members = { ...thread?.members, [id]: place.state };
    return [{ ids: new Set([id, ...(thread?.emailIds ?? [])]), members: Object.values(members) }];
  });
  return queries.map((query) => {
    const row = structuredClone(query);
    for (const { ids, members } of gone) {
      if (!members.some((member) => row.id in member.mailboxIds)) {
        strip(row, ids);
      }
    }
    return row;
  });
}

function putOf<Row>(change: { put?: readonly Row[] } | undefined): readonly Row[] {
  return change?.put ?? [];
}

// The batch with every pending change laid over the rows it lands, and
// whether a stored mailbox row moved with it.
async function overlaid(
  store: MailStore,
  accountId: string,
  { batch, kind }: { batch: Batch; kind: LandingKind },
  rows: readonly PendingRow[],
): Promise<{ batch: Batch; shifted: boolean }> {
  const held = byEmail(rows);
  const relaid = relaidHeaders(putOf(batch.emails), held);
  const { headers } = relaid;
  const standingHeaders = await known(headers, [...held.keys()], (ids) =>
    store.emails(accountId, ids),
  );
  const byHeader = new Map(relaid.fitted.map((row) => [row.seq, row]));
  const members = relaidThreads(
    putOf(batch.threads),
    byEmail(rows.map((row) => byHeader.get(row.seq) ?? row)),
    new Set(standingHeaders.keys()),
  );
  const fitted = [...relaid.fitted, ...members.fitted];
  const refit = new Map(fitted.map((row) => [row.seq, row]));
  const pending = byEmail(rows.map((row) => refit.get(row.seq) ?? row));
  const landing = { headers, threads: members.threads, mailboxes: putOf(batch.mailboxes) };
  const standing = await standingOf(store, accountId, landing, {
    headers: standingHeaders,
    pending,
  });
  const logs = { held, pending, shifts: kind === "round" && fitted.length > 0 };
  const { fresh, shifted } = await relaidMailboxes(store, accountId, { landing, standing }, logs);
  return {
    batch: {
      ...batch,
      emails: { ...batch.emails, put: headers },
      threads: { ...batch.threads, put: members.threads },
      mailboxes: { ...batch.mailboxes, put: [...fresh, ...shifted] },
      queries: { ...batch.queries, put: relaidLists(putOf(batch.queries), standing, pending) },
      pending: { ...batch.pending, put: [...putOf(batch.pending), ...fitted] },
    },
    shifted: shifted.length > 0,
  };
}

async function landAs(
  store: MailStore,
  accountId: string,
  batch: Batch,
  kind: LandingKind,
): Promise<boolean> {
  const rows = await store.pending(accountId);
  if (rows.length === 0) {
    await store.commit(accountId, batch);
    return false;
  }
  const laid = await overlaid(store, accountId, { batch, kind }, rows);
  await store.commit(accountId, laid.batch);
  return laid.shifted;
}

// Rows the server answered land with every pending change laid over
// them again, until the server acknowledges or refuses the change.
export async function land(store: MailStore, accountId: string, batch: Batch): Promise<void> {
  await landAs(store, accountId, batch, "fetch");
}

// The landing of one round of the change logs. True when a mailbox row
// the batch did not name moved, so the tree is read again.
export function landRound(store: MailStore, accountId: string, batch: Batch): Promise<boolean> {
  return landAs(store, accountId, batch, "round");
}
