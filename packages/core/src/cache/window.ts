// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor, outcome, reference } from "../jmap/calls";
import type { ObjectType, Start } from "../jmap/calls";
import { MethodFailure } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import {
  HEADER_PROPERTIES,
  STATE_PROPERTIES,
  emailHeaderSchema,
  getAnswerSchema,
  queryAnswerSchema,
  threadSchema,
} from "../jmap/schemas";
import type { EmailHeader, EmailState, Invocation } from "../jmap/schemas";
import { WINDOW_SIZE } from "./limits";
import { fetchStates, stateAnswerSchema, threadRow } from "./members";
import type { MailStore, QueryRow, ThreadRow } from "./store";

const headerAnswerSchema = getAnswerSchema(emailHeaderSchema);
const threadAnswerSchema = getAnswerSchema(threadSchema);

// What one window fetch brought: the exemplars in the server's order,
// their headers, their threads with every member's state and the
// states an account without any starts from.
export interface Fetched {
  ids: string[];
  total: number | null;
  queryState: string;
  emails: EmailHeader[];
  threads: ThreadRow[];
  states: Partial<Record<ObjectType, string>>;
}

// One page of a list as a caller renders it.
export interface WindowPage {
  ids: string[];
  total: number | null;
  // New mail the marker counts, held back until the user brings it in.
  pending: number;
  emails: Record<string, EmailHeader>;
  threads: Record<string, ThreadRow>;
}

interface Request {
  mailboxId: string;
  start: Start;
  calculateTotal: boolean;
}

interface Chunk {
  wanted: number;
  // Whether the account holds no states yet: two empty gets ahead of
  // the query take the states every later /changes starts from.
  leading: boolean;
}

export function pageOf(row: QueryRow | null, page: number): string[] | undefined {
  return row?.pages.find((held) => held.page === page)?.ids;
}

// A page held short of a full one while the total says rows follow it:
// it was the last page when it was fetched and the list grew since. The
// first page is the refresh's own and never reads as outgrown here.
function outgrown(row: QueryRow, page: number): boolean {
  const ids = pageOf(row, page);
  if (page === 0 || ids === undefined || row.total === null) {
    return false;
  }
  return ids.length < WINDOW_SIZE && page * WINDOW_SIZE + ids.length < row.total;
}

// A window of WINDOW_SIZE exemplars in one round trip: the query, the
// headers, the threads and the members' states by result reference
// (RFC 8620 section 3.7). A server that lowers the limit gets the rest
// asked by anchor until the page is full or the list ends.
async function fetchWindow(
  client: JmapClient,
  store: MailStore,
  request: Request,
): Promise<Fetched> {
  const known = await store.state(client.accountId, "Email");
  return fetchChunk(client, store, request, { wanted: WINDOW_SIZE, leading: known === null });
}

async function fetchChunk(
  client: JmapClient,
  store: MailStore,
  request: Request,
  chunk: Chunk,
): Promise<Fetched> {
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const responses = await client.request([
    ...(chunk.leading ? [calls.get("Email", [], "e0"), calls.get("Thread", [], "t0")] : []),
    calls.query({ ...request, limit: chunk.wanted }, "q"),
    calls.get("Email", reference("q", "Email/query", "/ids"), "h", HEADER_PROPERTIES),
    calls.get("Thread", reference("h", "Email/get", "/list/*/threadId"), "t"),
    calls.get("Email", reference("t", "Thread/get", "/list/*/emailIds"), "m", STATE_PROPERTIES),
  ]);
  const { lowered, ...fetched } = await read(client, store, responses, chunk.leading);
  const more = fetched.ids.length < chunk.wanted && lowered && fetched.ids.length > 0;
  const last = fetched.ids.at(-1);
  if (!more || last === undefined) {
    return fetched;
  }
  const rest = await fetchChunk(
    client,
    store,
    { ...request, start: { anchor: last, anchorOffset: 1 }, calculateTotal: false },
    { wanted: chunk.wanted - fetched.ids.length, leading: false },
  );
  return {
    ...fetched,
    ids: [...fetched.ids, ...rest.ids],
    emails: [...fetched.emails, ...rest.emails],
    threads: [...fetched.threads, ...rest.threads],
  };
}

// The answers of one chunk. Objects the query did not name are left
// out, ids the get did not find leave the window and a members answer
// that was too large is fetched in chunks.
async function read(
  client: JmapClient,
  store: MailStore,
  responses: readonly Invocation[],
  leading: boolean,
): Promise<Fetched & { lowered: boolean }> {
  const query = answer(responses, "q", queryAnswerSchema);
  const asked = new Set(query.ids);
  const emails = answer(responses, "h", headerAnswerSchema).list.filter((email) =>
    asked.has(email.id),
  );
  const found = new Set(emails.map((email) => email.id));
  const threadIds = new Set(emails.map((email) => email.threadId));
  const threads = answer(responses, "t", threadAnswerSchema).list.filter((thread) =>
    threadIds.has(thread.id),
  );
  const states = await membersOf(client, responses, threads);
  const held = await store.threads(client.accountId, [...threadIds]);
  return {
    ids: query.ids.filter((id) => found.has(id)),
    total: query.total ?? null,
    queryState: query.queryState,
    emails,
    threads: threads.map((thread) => threadRow(thread, states, held.get(thread.id))),
    states: leading
      ? {
          Email: answer(responses, "e0", stateAnswerSchema).state,
          Thread: answer(responses, "t0", threadAnswerSchema).state,
        }
      : {},
    lowered: query.limit !== undefined,
  };
}

async function membersOf(
  client: JmapClient,
  responses: readonly Invocation[],
  threads: readonly { emailIds: string[] }[],
): Promise<Map<string, EmailState>> {
  const members = outcome(responses, "m", stateAnswerSchema);
  if (members.status === "ok") {
    return new Map(members.value.list.map((state) => [state.id, state]));
  }
  if (members.type !== "requestTooLarge") {
    throw new MethodFailure(members.type, "m");
  }
  return fetchStates(
    client,
    threads.flatMap((thread) => thread.emailIds),
  );
}

// The page as the store holds it, fetched when it does not or when the
// list outgrew it: by anchor on the page before it, so the pages stay
// one list, by position when no page precedes it or the anchor left.
export async function queryWindow(
  client: JmapClient,
  store: MailStore,
  mailboxId: string,
  page: number,
): Promise<WindowPage> {
  const held = await store.query(client.accountId, mailboxId);
  if (held !== null && pageOf(held, page) !== undefined && !outgrown(held, page)) {
    return assemble(store, client.accountId, held, page);
  }
  const fetched = await fetchPage(client, store, { mailboxId, held, page });
  const row = withPage(held ?? emptyRow(mailboxId), page, fetched);
  await store.commit(client.accountId, {
    emails: { put: fetched.emails },
    threads: { put: fetched.threads },
    queries: { put: [row] },
    states: fetched.states,
  });
  return assemble(store, client.accountId, row, page);
}

function emptyRow(mailboxId: string): QueryRow {
  return { id: mailboxId, queryState: "", total: null, pages: [], pending: [], fresh: null };
}

// The row with the page; the total is the first page's. A page fetched
// anew takes the place of the one held, and the pages after it go with
// it, since they were anchored on its old end.
function withPage(row: QueryRow, page: number, fetched: Fetched): QueryRow {
  const kept =
    pageOf(row, page) === undefined ? row.pages : row.pages.filter((held) => held.page < page);
  return {
    ...row,
    queryState: fetched.queryState,
    total: page === 0 ? fetched.total : row.total,
    pages: [...kept, { page, ids: fetched.ids }],
  };
}

async function fetchPage(
  client: JmapClient,
  store: MailStore,
  { mailboxId, held, page }: { mailboxId: string; held: QueryRow | null; page: number },
): Promise<Fetched> {
  const byPosition: Request = {
    mailboxId,
    start: { position: page * WINDOW_SIZE },
    calculateTotal: page === 0,
  };
  const anchor = page > 0 ? pageOf(held, page - 1)?.at(-1) : undefined;
  if (anchor === undefined) {
    return fetchWindow(client, store, byPosition);
  }
  try {
    return await fetchWindow(client, store, {
      ...byPosition,
      start: { anchor, anchorOffset: 1 },
    });
  } catch (error) {
    if (error instanceof MethodFailure && error.type === "anchorNotFound") {
      return fetchWindow(client, store, byPosition);
    }
    throw error;
  }
}

async function assemble(
  store: MailStore,
  accountId: string,
  row: QueryRow,
  page: number,
): Promise<WindowPage> {
  const ids = pageOf(row, page) ?? [];
  const emails = await store.emails(accountId, ids);
  const threadIds = [...new Set([...emails.values()].map((email) => email.threadId))];
  const threads = await store.threads(accountId, threadIds);
  return {
    ids: ids.filter((id) => emails.has(id)),
    total: row.total,
    pending: row.pending.length,
    emails: Object.fromEntries(emails),
    threads: Object.fromEntries(threads),
  };
}

// The first page as the server orders it now, with the total.
export function fetchFirstPage(
  client: JmapClient,
  store: MailStore,
  mailboxId: string,
): Promise<Fetched> {
  return fetchWindow(client, store, { mailboxId, start: { position: 0 }, calculateTotal: true });
}

export type Filler = Pick<Fetched, "ids" | "emails" | "threads">;

export function nothingFetched(): Filler {
  return { ids: [], emails: [], threads: [] };
}

// `wanted` rows after the row `after`.
export interface Gap {
  mailboxId: string;
  after: string | undefined;
  wanted: number;
}

// The rows after an anchor, by anchor on it; none when nothing is asked
// or the anchor left the list.
export async function fetchAfter(client: JmapClient, store: MailStore, gap: Gap): Promise<Filler> {
  if (gap.wanted <= 0 || gap.after === undefined) {
    return nothingFetched();
  }
  try {
    return await fetchChunk(
      client,
      store,
      {
        mailboxId: gap.mailboxId,
        start: { anchor: gap.after, anchorOffset: 1 },
        calculateTotal: false,
      },
      { wanted: gap.wanted, leading: false },
    );
  } catch (error) {
    if (error instanceof MethodFailure && error.type === "anchorNotFound") {
      return nothingFetched();
    }
    throw error;
  }
}
