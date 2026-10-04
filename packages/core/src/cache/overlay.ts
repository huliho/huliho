// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import { memberOf } from "./members";
import { replayed, unpatched } from "./patch";
import type { Batch, MailStore, MemberState, PendingRow, ThreadRow } from "./store";
import { movesOf, recounted, shiftOf } from "./unread";
import type { MemberChange, Moves } from "./unread";

type Pending = Map<string, PendingRow[]>;

// Where rows come from. A round of the change logs brings every mailbox
// up to date within its poll; a fetch of a list or a thread brings none.
type LandingKind = "round" | "fetch";

// The rows a landing writes that the unread counts read.
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
  members: Map<string, Record<string, MemberState>>;
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
    const replay = replayed(header.keywords, rows);
    fitted.push(...refitted(rows, replay.rows));
    return { ...header, keywords: replay.keywords };
  });
  return { headers: relaid, fitted };
}

function relaidMember(id: string, member: MemberState, pending: Pending): MemberState {
  const rows = pending.get(id);
  return rows === undefined
    ? member
    : { ...member, keywords: replayed(member.keywords, rows).keywords };
}

function relaidThreads(threads: readonly ThreadRow[], pending: Pending): ThreadRow[] {
  return threads.map((thread) => ({
    ...thread,
    members: Object.fromEntries(
      Object.entries(thread.members).map(([id, member]) => [id, relaidMember(id, member, pending)]),
    ),
  }));
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

async function standingOf(
  store: MailStore,
  accountId: string,
  landing: Landed,
  emailIds: readonly string[],
): Promise<Standing> {
  const headers = await known(landing.headers, emailIds, (ids) => store.emails(accountId, ids));
  const threadIds = [...new Set([...headers.values()].map((header) => header.threadId))];
  const threads = await known(landing.threads, threadIds, (ids) => store.threads(accountId, ids));
  return { headers, members: new Map([...threads].map(([id, thread]) => [id, thread.members])) };
}

// How far the pending rows move the unread counts away from what the
// server holds, which their inverses say.
function pendingMoves({ headers, members }: Standing, pending: Pending): Moves {
  const changes = [...pending].flatMap(([id, rows]): MemberChange[] => {
    const header = headers.get(id);
    if (header === undefined) {
      return [];
    }
    const after = memberOf(header);
    const before = { ...after, keywords: unpatched(header.keywords, rows) };
    return [{ id, threadId: header.threadId, before, after }];
  });
  return movesOf(members, changes);
}

// The mailbox rows a landing writes. A row that lands carries the
// server's count, so the pending moves are counted in again; in a round
// a row that does not land takes the difference a refit makes.
async function relaidMailboxes(
  store: MailStore,
  accountId: string,
  landing: Landed,
  logs: { held: Pending; pending: Pending; shifts: boolean },
): Promise<Recount> {
  if (landing.mailboxes.length === 0 && !logs.shifts) {
    return { fresh: [], shifted: [] };
  }
  const standing = await standingOf(store, accountId, landing, [...logs.pending.keys()]);
  const moved = pendingMoves(standing, logs.pending);
  const fresh = landing.mailboxes.map((row) => recounted([row], moved).at(0) ?? row);
  if (!logs.shifts) {
    return { fresh, shifted: [] };
  }
  const landed = new Set(landing.mailboxes.map((row) => row.id));
  const stored = (await store.mailboxes(accountId)).filter((row) => !landed.has(row.id));
  return { fresh, shifted: recounted(stored, shiftOf(pendingMoves(standing, logs.held), moved)) };
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
  const { headers, fitted } = relaidHeaders(putOf(batch.emails), held);
  const refit = new Map(fitted.map((row) => [row.seq, row]));
  const pending = byEmail(rows.map((row) => refit.get(row.seq) ?? row));
  const threads = relaidThreads(putOf(batch.threads), pending);
  const landed = { headers, threads, mailboxes: putOf(batch.mailboxes) };
  const logs = { held, pending, shifts: kind === "round" && fitted.length > 0 };
  const { fresh, shifted } = await relaidMailboxes(store, accountId, landed, logs);
  return {
    batch: {
      ...batch,
      emails: { ...batch.emails, put: headers },
      threads: { ...batch.threads, put: threads },
      mailboxes: { ...batch.mailboxes, put: [...fresh, ...shifted] },
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
