// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { authenticationOf } from "../auth-results";
import { BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE, bodyAnswerSchema } from "../jmap/body";
import type { BodyAnswer, EmailBodyValue } from "../jmap/body";
import { callsFor, outcome } from "../jmap/calls";
import { JmapError, MethodFailure, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import { getAnswerSchema } from "../jmap/schemas";
import { flowedOf } from "../text/flowed";
import { BODY_CACHE_BYTES, BODY_CACHE_ROWS } from "./limits";
import type { EmailBody, MailStore } from "./store";

const answerSchema = getAnswerSchema(bodyAnswerSchema);

function isCut(body: EmailBody): boolean {
  return Object.values(body.bodyValues).some((value) => value.isTruncated);
}

// The UTF-8 weight of the values, which is what a store keeps of a body.
function weightOf(values: Record<string, EmailBodyValue>): number {
  const encoder = new TextEncoder();
  return Object.values(values).reduce((sum, part) => sum + encoder.encode(part.value).length, 0);
}

function rowOf(answer: BodyAnswer, large: boolean): EmailBody {
  return {
    id: answer.id,
    bodyStructure: answer.bodyStructure,
    textBody: answer.textBody,
    htmlBody: answer.htmlBody,
    attachments: answer.attachments,
    bodyValues: answer.bodyValues,
    authentication: authenticationOf(answer["header:Authentication-Results:asRaw:all"]),
    flowed: flowedOf(answer["header:Content-Type:asRaw"]),
    large,
    fetchedAt: Date.now(),
    bytes: weightOf(answer.bodyValues),
  };
}

// One Email/get for one email: both body alternatives with their
// values, so a later look at the plain text asks nothing. Null for an
// email the server does not have.
async function fetchBody(
  client: JmapClient,
  emailId: string,
  large: boolean,
): Promise<EmailBody | null> {
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const cap = large ? BODY_VALUE_BYTES_LARGE : BODY_VALUE_BYTES;
  const responses = await client.request([calls.body(emailId, cap, "b")]);
  const read = outcome(responses, "b", answerSchema);
  if (read.status === "error") {
    throw read.type === SERVER_UNAVAILABLE
      ? new JmapError("unavailable")
      : new MethodFailure(read.type, "b");
  }
  const answer = read.value.list.find((row) => row.id === emailId);
  return answer === undefined ? null : rowOf(answer, large);
}

// The bodies fetched longest ago leave, across every account, until the
// store is inside both bounds.
async function evict(store: MailStore): Promise<void> {
  const sizes = (await store.bodySizes()).toSorted((a, b) => a.fetchedAt - b.fetchedAt);
  let rows = sizes.length;
  let bytes = sizes.reduce((sum, size) => sum + size.bytes, 0);
  const leaving = new Map<string, string[]>();
  for (const size of sizes) {
    if (rows <= BODY_CACHE_ROWS && bytes <= BODY_CACHE_BYTES) {
      break;
    }
    rows -= 1;
    bytes -= size.bytes;
    leaving.set(size.accountId, [...(leaving.get(size.accountId) ?? []), size.id]);
  }
  await Promise.all(
    [...leaving].map(([accountId, remove]) => store.commit(accountId, { bodies: { remove } })),
  );
}

// The body of one email: the row the store holds, else one request
// whose answer is stored. `large` asks again at the large cap for a
// body the first ask cut short. Null for an email the server does not
// have.
export async function readBody(
  client: JmapClient,
  store: MailStore,
  emailId: string,
  { large }: { large: boolean },
): Promise<EmailBody | null> {
  const { accountId } = client;
  const held = await store.body(accountId, emailId);
  if (held !== null && (held.large || !large || !isCut(held))) {
    return held;
  }
  const fetched = await fetchBody(client, emailId, large);
  if (fetched === null) {
    await store.commit(accountId, { bodies: { remove: [emailId] } });
    return null;
  }
  await store.commit(accountId, { bodies: { put: [fetched] } });
  await evict(store);
  return fetched;
}
