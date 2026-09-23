// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { z } from "../schema";
import { MethodFailure } from "./client";
import type { MethodCall } from "./client";
import { methodErrorSchema } from "./schemas";
import type { Invocation } from "./schemas";

// The object types the cache holds, as their methods name them.
export type ObjectType = "Mailbox" | "Email" | "Thread";

// A result reference (RFC 8620 section 3.7): the value an earlier
// response holds at a path.
export interface Reference {
  resultOf: string;
  name: string;
  path: string;
}

// Where the ids of a query window start (RFC 8620 section 5.5).
export type Start = { position: number } | { anchor: string; anchorOffset: number };

export function reference(resultOf: string, name: string, path: string): Reference {
  return { resultOf, name, path };
}

// The ids of a call: listed, taken from an earlier response or, for a
// /get that may take it, every object.
function idsArgument(ids: readonly string[] | Reference | null): Record<string, unknown> {
  if (ids !== null && "resultOf" in ids) {
    return { "#ids": ids };
  }
  return { ids };
}

// The method calls of one account, each carrying its account id.
export interface Calls {
  get(
    type: ObjectType,
    ids: readonly string[] | Reference | null,
    id: string,
    properties?: readonly string[],
  ): MethodCall;
  query(
    window: { mailboxId: string; start: Start; limit: number; calculateTotal: boolean },
    id: string,
  ): MethodCall;
  changes(type: ObjectType, sinceState: string, id: string): MethodCall;
}

export function callsFor(accountId: string): Calls {
  return {
    get: (type, ids, id, properties) => ({
      name: `${type}/get`,
      arguments: {
        accountId,
        ...idsArgument(ids),
        ...(properties === undefined ? {} : { properties }),
      },
      id,
    }),
    // The list of one mailbox, newest first, one email per thread.
    query: (window, id) => ({
      name: "Email/query",
      arguments: {
        accountId,
        filter: { inMailbox: window.mailboxId },
        sort: [{ property: "receivedAt", isAscending: false }],
        ...window.start,
        limit: window.limit,
        calculateTotal: window.calculateTotal,
        collapseThreads: true,
      },
      id,
    }),
    changes: (type, sinceState, id) => ({
      name: `${type}/changes`,
      arguments: { accountId, sinceState },
      id,
    }),
  };
}

// What one call came back with: its value, or the error type the
// server answered instead.
export type Outcome<Value> = { status: "ok"; value: Value } | { status: "error"; type: string };

// The response to a call, read through its schema. A server answers
// every call, so a missing response is a broken one.
export function outcome<Value>(
  responses: readonly Invocation[],
  call: string,
  schema: z.ZodType<Value>,
): Outcome<Value> {
  const found = responses.find(([, , id]) => id === call);
  if (found === undefined) {
    throw new Error(`the server answered no response to call ${call}`);
  }
  const [name, value] = found;
  if (name === "error") {
    return { status: "error", type: methodErrorSchema.parse(value).type };
  }
  return { status: "ok", value: schema.parse(value) };
}

// The value of a call the caller cannot do without.
export function answer<Value>(
  responses: readonly Invocation[],
  call: string,
  schema: z.ZodType<Value>,
): Value {
  const read = outcome(responses, call, schema);
  if (read.status === "error") {
    throw new MethodFailure(read.type, call);
  }
  return read.value;
}
