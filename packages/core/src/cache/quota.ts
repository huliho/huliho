// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { callsFor, reference } from "../jmap/calls";
import type { Calls, Outcome } from "../jmap/calls";
import type { JmapClient, MethodCall } from "../jmap/client";
import {
  changesAnswerSchema,
  countSchema,
  getAnswerSchema,
  idSchema,
  methodErrorSchema,
} from "../jmap/schemas";
import type { Invocation } from "../jmap/schemas";
import { z } from "../schema";
import type { Batch, MailStore } from "./store";

// A share of the hard limit past which the bar warns, where the server
// names no warn limit of its own.
export const QUOTA_WARN_RATIO = 0.9;

// The type names a quota of mail carries: RFC 9425 section 4.1 names
// Email, its own example in section 5.1 writes Mail.
const MAIL_TYPES: ReadonlySet<string> = new Set(["Email", "Mail"]);

// One Quota object (RFC 9425 section 4.1).
const quotaSchema = z.object({
  id: idSchema,
  resourceType: z.string(),
  used: countSchema,
  hardLimit: countSchema,
  scope: z.string(),
  name: z.string(),
  types: z.array(z.string()),
  warnLimit: z.nullish(countSchema),
  softLimit: z.nullish(countSchema),
  description: z.nullish(z.string()),
});

export type Quota = z.infer<typeof quotaSchema>;

// The quotas of an account as the server last answered them, with the
// state they stand at.
export const heldQuotaSchema = z.object({ state: z.string(), list: z.array(quotaSchema) });

export type HeldQuota = z.infer<typeof heldQuotaSchema>;

// What the bar shows: the quota's unit, how much is used of how much,
// the used share capped at one and whether it warns.
export interface QuotaReading {
  unit: "octets" | "count";
  used: number;
  limit: number;
  share: number;
  warn: boolean;
}

function readingOf(quota: Quota, unit: QuotaReading["unit"]): QuotaReading {
  const warnAt = quota.warnLimit ?? QUOTA_WARN_RATIO * quota.hardLimit;
  return {
    unit,
    used: quota.used,
    limit: quota.hardLimit,
    share: Math.min(1, quota.used / quota.hardLimit),
    warn: quota.used >= warnAt,
  };
}

// The fullest quota of the account's mail in one unit; a limit of zero
// leaves no share to show.
function fullest(list: readonly Quota[], unit: QuotaReading["unit"]): QuotaReading | null {
  const readings = list
    .filter(
      (quota) =>
        quota.resourceType === unit &&
        quota.scope === "account" &&
        quota.hardLimit > 0 &&
        quota.types.some((type) => MAIL_TYPES.has(type)),
    )
    .map((quota) => readingOf(quota, unit));
  return readings.reduce<QuotaReading | null>(
    (best, reading) => (best === null || reading.share > best.share ? reading : best),
    null,
  );
}

// The quota the bar shows: the fullest one in octets, else the fullest
// one in messages, else none.
export function pickQuota(list: readonly Quota[]): QuotaReading | null {
  return fullest(list, "octets") ?? fullest(list, "count");
}

export async function readQuota(store: MailStore, accountId: string): Promise<QuotaReading | null> {
  const held = await store.quota(accountId);
  return held === null ? null : pickQuota(held.list);
}

const quotaAnswerSchema = getAnswerSchema(quotaSchema);

const WHOLE = "q:Quota";
const CHANGES = "c:Quota";
const CREATED = "n:Quota";
const UPDATED = "u:Quota";

// The quota calls a mailbox pass carries: the whole list while none is
// held, else the changes since with the quotas they name.
function quotaCalls(calls: Calls, held: HeldQuota | null): MethodCall[] {
  if (held === null) {
    return [calls.get("Quota", null, WHOLE)];
  }
  return [
    calls.changes("Quota", held.state, CHANGES),
    calls.get("Quota", reference(CHANGES, "Quota/changes", "/created"), CREATED),
    calls.get("Quota", reference(CHANGES, "Quota/changes", "/updated"), UPDATED),
  ];
}

// The account answers no quota; it holds none then.
const NOT_SUPPORTED = "accountNotSupportedByMethod";

// The word for an answer the client cannot read.
const UNREADABLE = "unreadable";

// The answer to one quota call. A missing response or one off its
// schema reads as a refusal, so a quota the client cannot read never
// stops the pass that carries it.
function readOf<Value>(
  responses: readonly Invocation[],
  call: string,
  schema: z.ZodMiniType<Value>,
): Outcome<Value> {
  const found = responses.find(([, , id]) => id === call);
  if (found === undefined) {
    return { status: "error", type: UNREADABLE };
  }
  const [name, value] = found;
  if (name === "error") {
    const error = methodErrorSchema.safeParse(value);
    return { status: "error", type: error.success ? error.data.type : UNREADABLE };
  }
  const parsed = schema.safeParse(value);
  return parsed.success
    ? { status: "ok", value: parsed.data }
    : { status: "error", type: UNREADABLE };
}

// What a list answer makes of the held quotas. Any other refusal keeps
// what is held.
function wholeOf(responses: readonly Invocation[], held: HeldQuota | null): HeldQuota | null {
  const read = readOf(responses, WHOLE, quotaAnswerSchema);
  if (read.status === "ok") {
    return { state: read.value.state, list: read.value.list };
  }
  return read.type === NOT_SUPPORTED ? null : held;
}

// What the changes make of the held quotas; "refetch" when the server
// cannot say what changed since (RFC 8620 section 5.2).
function changedOf(
  responses: readonly Invocation[],
  held: HeldQuota,
): HeldQuota | null | "refetch" {
  const changes = readOf(responses, CHANGES, changesAnswerSchema);
  if (changes.status === "error") {
    if (changes.type === "cannotCalculateChanges") {
      return "refetch";
    }
    return changes.type === NOT_SUPPORTED ? null : held;
  }
  const created = readOf(responses, CREATED, quotaAnswerSchema);
  const updated = readOf(responses, UPDATED, quotaAnswerSchema);
  if (changes.value.hasMoreChanges || created.status === "error" || updated.status === "error") {
    return "refetch";
  }
  const named = new Map([...created.value.list, ...updated.value.list].map((row) => [row.id, row]));
  const gone = new Set(changes.value.destroyed);
  const kept = held.list.filter((row) => !gone.has(row.id));
  const known = new Set(kept.map((row) => row.id));
  return {
    state: changes.value.newState,
    list: [
      ...kept.map((row) => named.get(row.id) ?? row),
      ...[...named.values()].filter((row) => !known.has(row.id)),
    ],
  };
}

// The held quotas after a mailbox pass whose answers are `responses`,
// asked as the changes since `asked` or whole when it is null; the
// whole list is asked again where the changes cannot be followed.
async function quotaAfter(
  client: JmapClient,
  responses: readonly Invocation[],
  asked: HeldQuota | null,
  held: HeldQuota | null,
): Promise<HeldQuota | null> {
  const next = asked === null ? wholeOf(responses, held) : changedOf(responses, asked);
  if (next !== "refetch") {
    return next;
  }
  const session = await client.session();
  const again = await client.request([callsFor(session.accountId).get("Quota", null, WHOLE)]);
  return wholeOf(again, held);
}

// What the quota part of a pass lands: the quotas when anything differs
// and whether their list moved.
export interface QuotaLanding {
  batch: Pick<Batch, "quota">;
  moved: boolean;
}

// The quota part of a mailbox pass: the calls that ride its request and
// what their answers land. An account that lists the quota capability
// is asked; one that does not holds none (RFC 9425 section 2.1).
// `whole` asks the whole list whatever is held.
export interface QuotaPass {
  calls: MethodCall[];
  land(responses: readonly Invocation[]): Promise<QuotaLanding>;
}

// The new state is stored whenever anything differs; the tabs read the
// tree again only when the list moved, a list that appears or goes
// included.
function landingOf(held: HeldQuota | null, next: HeldQuota | null): QuotaLanding {
  if (JSON.stringify(next) === JSON.stringify(held)) {
    return { batch: {}, moved: false };
  }
  const moved = JSON.stringify(held?.list ?? null) !== JSON.stringify(next?.list ?? null);
  return { batch: { quota: next }, moved };
}

export async function quotaPass(
  client: JmapClient,
  store: MailStore,
  whole: boolean,
): Promise<QuotaPass> {
  const session = await client.session();
  const held = await store.quota(client.accountId);
  if (!session.quota) {
    return { calls: [], land: () => Promise.resolve(landingOf(held, null)) };
  }
  const asked = whole ? null : held;
  return {
    calls: quotaCalls(callsFor(session.accountId), asked),
    land: async (responses) => landingOf(held, await quotaAfter(client, responses, asked, held)),
  };
}
