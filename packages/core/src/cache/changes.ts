// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor, outcome, reference } from "../jmap/calls";
import type { Calls, ObjectType } from "../jmap/calls";
import { MethodFailure } from "../jmap/client";
import type { JmapClient, MethodCall } from "../jmap/client";
import {
  STATE_PROPERTIES,
  changesAnswerSchema,
  emailStateSchema,
  getAnswerSchema,
  mailboxSchema,
  threadSchema,
} from "../jmap/schemas";
import type { ChangesAnswer, EmailState, Invocation, Mailbox, Thread } from "../jmap/schemas";
import type { z } from "../schema";
import { landEmails, landThreads, mergeFolded, nothingFolded } from "./landing";
import type { Folded, Held, Landing } from "./landing";
import { fetchInChunks, fetchStates } from "./members";
import type { MailStore } from "./store";

const mailboxAnswerSchema = getAnswerSchema(mailboxSchema);
const stateAnswerSchema = getAnswerSchema(emailStateSchema);
const threadAnswerSchema = getAnswerSchema(threadSchema);

interface Plan {
  type: ObjectType;
  since: string;
}

// The changes of one type with the objects they name; a null list is
// one the server would not answer in one call, fetched in chunks then.
interface Mailboxes {
  changes: ChangesAnswer;
  put: Mailbox[] | null;
}

interface Emails {
  changes: ChangesAnswer;
  states: EmailState[] | null;
}

interface Threads {
  changes: ChangesAnswer;
  threads: Thread[] | null;
  members: EmailState[] | null;
}

// The answers of one type, or the word that its state is past the
// server's horizon.
type Read<Value> = Value | "refetch" | null;

interface Parsed {
  mailbox: Read<Mailboxes>;
  email: Read<Emails>;
  thread: Read<Threads>;
}

// Every mailbox of the account and the state they stand at.
export async function hydrateMailboxes(client: JmapClient, store: MailStore): Promise<void> {
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const responses = await client.request([calls.get("Mailbox", null, "m")]);
  const got = answer(responses, "m", mailboxAnswerSchema);
  await store.commit(client.accountId, {
    reset: ["mailboxes"],
    mailboxes: { put: got.list },
    states: { Mailbox: got.state },
  });
}

// The changes since the states the store holds for `types`, applied in
// one batch per round; a type without a state is left alone.
export async function followChanges(
  client: JmapClient,
  store: MailStore,
  types: readonly ObjectType[],
  rounds: number,
): Promise<Folded> {
  const plans = await plansFor(store, client.accountId, types);
  if (plans.length === 0) {
    return nothingFolded();
  }
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const responses = await client.request(plans.flatMap((plan) => callsOf(calls, plan)));
  const parsed = parse(plans, responses);
  const folded = await apply(client, store, parsed);
  const more = plans.filter((plan) => hasMore(parsed, plan.type)).map((plan) => plan.type);
  if (more.length === 0 || rounds <= 1) {
    return folded;
  }
  return mergeFolded(folded, await followChanges(client, store, more, rounds - 1));
}

async function plansFor(
  store: MailStore,
  accountId: string,
  types: readonly ObjectType[],
): Promise<Plan[]> {
  const held = await Promise.all(
    types.map(async (type) => ({ type, since: await store.state(accountId, type) })),
  );
  return held.flatMap(({ type, since }) => (since === null ? [] : [{ type, since }]));
}

// The calls of one type: its changes and the objects they name, the
// members of an updated thread included (RFC 8620 section 3.7).
function callsOf(calls: Calls, { type, since }: Plan): MethodCall[] {
  const changes = `c:${type}`;
  const created = reference(changes, `${type}/changes`, "/created");
  const updated = reference(changes, `${type}/changes`, "/updated");
  if (type === "Mailbox") {
    return [
      calls.changes(type, since, changes),
      calls.get(type, created, "n:Mailbox"),
      calls.get(type, updated, "u:Mailbox"),
    ];
  }
  if (type === "Email") {
    return [
      calls.changes(type, since, changes),
      calls.get(type, created, "n:Email", STATE_PROPERTIES),
      calls.get(type, updated, "u:Email", STATE_PROPERTIES),
    ];
  }
  return [
    calls.changes(type, since, changes),
    calls.get(type, updated, "u:Thread"),
    calls.get(
      "Email",
      reference("u:Thread", "Thread/get", "/list/*/emailIds"),
      "m:Thread",
      STATE_PROPERTIES,
    ),
  ];
}

// The changes of one type; a state past the horizon reads as a refetch
// and any other refusal is the caller's.
function changesOf(responses: readonly Invocation[], type: ObjectType): ChangesAnswer | "refetch" {
  const read = outcome(responses, `c:${type}`, changesAnswerSchema);
  if (read.status === "ok") {
    return read.value;
  }
  if (read.type === "cannotCalculateChanges") {
    return "refetch";
  }
  throw new MethodFailure(read.type, `c:${type}`);
}

// The objects a get by reference answered, null when there were too
// many for one answer; any other refusal is the caller's.
function listed<Schema extends z.ZodType<{ list: unknown[] }>>(
  responses: readonly Invocation[],
  call: string,
  schema: Schema,
): z.output<Schema>["list"] | null {
  const read = outcome(responses, call, schema);
  if (read.status === "ok") {
    return read.value.list;
  }
  if (read.type === "requestTooLarge") {
    return null;
  }
  throw new MethodFailure(read.type, call);
}

// The lists of created and updated objects as one, or null when either
// was too large.
function both<Item>(created: Item[] | null, updated: Item[] | null): Item[] | null {
  return created === null || updated === null ? null : [...created, ...updated];
}

function parse(plans: readonly Plan[], responses: readonly Invocation[]): Parsed {
  const planned = new Set(plans.map((plan) => plan.type));
  return {
    mailbox: planned.has("Mailbox") ? parseMailboxes(responses) : null,
    email: planned.has("Email") ? parseEmails(responses) : null,
    thread: planned.has("Thread") ? parseThreads(responses) : null,
  };
}

function parseMailboxes(responses: readonly Invocation[]): Read<Mailboxes> {
  const changes = changesOf(responses, "Mailbox");
  if (changes === "refetch") {
    return changes;
  }
  const put = both(
    listed(responses, "n:Mailbox", mailboxAnswerSchema),
    listed(responses, "u:Mailbox", mailboxAnswerSchema),
  );
  return { changes, put };
}

function parseEmails(responses: readonly Invocation[]): Read<Emails> {
  const changes = changesOf(responses, "Email");
  if (changes === "refetch") {
    return changes;
  }
  const states = both(
    listed(responses, "n:Email", stateAnswerSchema),
    listed(responses, "u:Email", stateAnswerSchema),
  );
  return { changes, states };
}

function parseThreads(responses: readonly Invocation[]): Read<Threads> {
  const changes = changesOf(responses, "Thread");
  if (changes === "refetch") {
    return changes;
  }
  return {
    changes,
    threads: listed(responses, "u:Thread", threadAnswerSchema),
    members: listed(responses, "m:Thread", stateAnswerSchema),
  };
}

function readOf(parsed: Parsed, type: ObjectType): Read<{ changes: ChangesAnswer }> {
  if (type === "Mailbox") {
    return parsed.mailbox;
  }
  if (type === "Email") {
    return parsed.email;
  }
  return parsed.thread;
}

function hasMore(parsed: Parsed, type: ObjectType): boolean {
  const read = readOf(parsed, type);
  return read !== null && read !== "refetch" && read.changes.hasMoreChanges;
}

// The round lands in one batch. A state past the horizon on emails or
// threads drops every email, thread and list, so the next window fetch
// starts afresh; mailboxes are fetched whole in their own request.
async function apply(client: JmapClient, store: MailStore, parsed: Parsed): Promise<Folded> {
  const { accountId } = client;
  const landing: Landing = { batch: {}, folded: nothingFolded() };
  if (parsed.mailbox !== null && parsed.mailbox !== "refetch") {
    await landMailboxes(client, parsed.mailbox, landing);
  }
  if (parsed.email === "refetch" || parsed.thread === "refetch") {
    landing.batch.reset = ["emails", "threads", "queries"];
    landing.batch.states = { ...landing.batch.states, Email: null, Thread: null };
    landing.folded.lists = (await store.queries(accountId)).map((row) => row.id);
    landing.folded.reset = true;
    landing.folded.listMoved = true;
  } else {
    await applyObjects(client, store, parsed, landing);
  }
  await store.commit(accountId, landing.batch);
  if (parsed.mailbox === "refetch") {
    await hydrateMailboxes(client, store);
    landing.folded.mailboxes = true;
  }
  return landing.folded;
}

async function landMailboxes(
  client: JmapClient,
  mailbox: Mailboxes,
  { batch, folded }: Landing,
): Promise<void> {
  const { changes } = mailbox;
  const named = new Set([...changes.created, ...changes.updated]);
  const put =
    mailbox.put ??
    (await fetchInChunks(client, { type: "Mailbox", ids: [...named] }, mailboxSchema));
  batch.mailboxes = { put: put.filter((row) => named.has(row.id)), remove: changes.destroyed };
  batch.states = { Mailbox: changes.newState };
  folded.mailboxes = named.size > 0 || changes.destroyed.length > 0;
}

async function applyObjects(
  client: JmapClient,
  store: MailStore,
  parsed: Parsed,
  landing: Landing,
): Promise<void> {
  const email = parsed.email === "refetch" ? null : parsed.email;
  const thread = parsed.thread === "refetch" ? null : parsed.thread;
  const states = await statesOf(client, email);
  const threads = await threadsOf(client, thread);
  const members = await membersOf(client, thread, threads);
  const held = await readHeld(client, store, states, threads);
  if (email !== null) {
    landEmails(email.changes, states, held, landing);
  }
  if (thread !== null) {
    landThreads({ changes: thread.changes, threads, members }, held.heldThreads, landing);
  }
  landing.batch.threads = { ...landing.batch.threads, put: [...held.heldThreads.values()] };
}

// The states of the created and updated emails, kept to the ones named.
async function statesOf(client: JmapClient, email: Emails | null): Promise<EmailState[]> {
  if (email === null) {
    return [];
  }
  const named = new Set([...email.changes.created, ...email.changes.updated]);
  const states =
    email.states ??
    (await fetchInChunks(
      client,
      { type: "Email", ids: [...named], properties: STATE_PROPERTIES },
      emailStateSchema,
    ));
  return states.filter((state) => named.has(state.id));
}

async function threadsOf(client: JmapClient, thread: Threads | null): Promise<Thread[]> {
  if (thread === null) {
    return [];
  }
  const updated = new Set(thread.changes.updated);
  const threads =
    thread.threads ??
    (await fetchInChunks(client, { type: "Thread", ids: [...updated] }, threadSchema));
  return threads.filter((row) => updated.has(row.id));
}

async function membersOf(
  client: JmapClient,
  thread: Threads | null,
  threads: readonly Thread[],
): Promise<Map<string, EmailState>> {
  if (thread === null) {
    return new Map();
  }
  if (thread.members !== null) {
    return new Map(thread.members.map((state) => [state.id, state]));
  }
  return fetchStates(
    client,
    threads.flatMap((row) => row.emailIds),
  );
}

async function readHeld(
  client: JmapClient,
  store: MailStore,
  states: readonly EmailState[],
  threads: readonly Thread[],
): Promise<Held> {
  const threadIds = [...states.map((state) => state.threadId), ...threads.map((row) => row.id)];
  const [headers, heldThreads, queries] = await Promise.all([
    store.emails(
      client.accountId,
      states.map((state) => state.id),
    ),
    store.threads(client.accountId, threadIds),
    store.queries(client.accountId),
  ]);
  return { headers, heldThreads, queries };
}
