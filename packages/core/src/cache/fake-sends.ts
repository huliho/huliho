// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The sending side of the fake, as the e2e mock answers it: the account's
// identities (RFC 8621 section 6), EmailSubmission/set with the
// implicit Email/set behind it (section 7.5) and a scripted refusal.

import type { FakeJmap } from "./fake-jmap";
import { MethodError, UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";
import { emailSet } from "./fake-writes";

// The account's own identity, the one a server answers by default.
export const IDENTITY = {
  id: "identity-1",
  name: "",
  email: "sanne@example.test",
  replyTo: null,
  bcc: null,
  textSignature: "",
  htmlSignature: "",
  mayDelete: false,
};
const IDENTITY_STATE = "i1";

function isRecord(value: unknown): value is Args {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function identityGet(server: FakeJmap, raw: Args): Args {
  if (raw["accountId"] !== UPSTREAM) {
    throw new MethodError("accountNotFound");
  }
  return { accountId: UPSTREAM, state: IDENTITY_STATE, list: server.identities, notFound: [] };
}

// The email a submission names, a creation id of the request read
// through what the request made (RFC 8620 section 5.3).
function emailIdOf(object: Args, made: ReadonlyMap<string, string>): string {
  const given = object["emailId"];
  const named = typeof given === "string" ? given : "";
  return named.startsWith("#") ? (made.get(named.slice(1)) ?? named) : named;
}

// What the server refuses in a submission before anything goes out.
function submissionError(server: FakeJmap, object: Args, emailId: string): Args | null {
  if (server.sendRefusal !== null) {
    return { ...server.sendRefusal };
  }
  if (!server.identities.some((identity) => identity.id === object["identityId"])) {
    return { type: "invalidProperties", properties: ["identityId"] };
  }
  const email = server.emails.get(emailId);
  if (email === undefined) {
    return { type: "invalidProperties", properties: ["emailId"] };
  }
  const recipients = [...(email.to ?? []), ...(email.cc ?? []), ...(email.bcc ?? [])];
  return recipients.length > 0 ? null : { type: "noRecipients" };
}

// The implicit Email/set: each sent submission's patch applied to its email.
function implicitSet(server: FakeJmap, raw: Args, sent: ReadonlyMap<string, string>): Args {
  const patches = isRecord(raw["onSuccessUpdateEmail"]) ? raw["onSuccessUpdateEmail"] : {};
  const update = Object.fromEntries(
    Object.entries(patches).flatMap(([key, patch]) => {
      const emailId = sent.get(key);
      return emailId === undefined ? [] : [[emailId, patch]];
    }),
  );
  return emailSet(server, { accountId: UPSTREAM, update });
}

// EmailSubmission/set: each create the server takes goes out at once,
// final; then the implicit Email/set where the call asks for one.
export function submissionSet(
  server: FakeJmap,
  raw: Args,
  made: Map<string, string>,
): [string, Args][] {
  if (server.sendMethodError !== null) {
    throw new MethodError(server.sendMethodError);
  }
  const creates = isRecord(raw["create"]) ? Object.entries(raw["create"]) : [];
  const created: [string, Args][] = [];
  const notCreated: [string, Args][] = [];
  const sent = new Map<string, string>();
  for (const [key, object] of creates) {
    const asked = isRecord(object) ? object : {};
    const emailId = emailIdOf(asked, made);
    const refusal = submissionError(server, asked, emailId);
    if (refusal !== null) {
      notCreated.push([key, refusal]);
      continue;
    }
    server.submissions.push({ ...asked, emailId });
    const id = `submission-${String(server.submissions.length)}`;
    sent.set(`#${key}`, emailId);
    created.push([key, { id, undoStatus: "final", sendAt: new Date().toISOString() }]);
  }
  const answer = {
    accountId: UPSTREAM,
    oldState: String(server.submissions.length - created.length),
    newState: String(server.submissions.length),
    created: created.length > 0 ? Object.fromEntries(created) : null,
    notCreated: notCreated.length > 0 ? Object.fromEntries(notCreated) : null,
  };
  const implicit: [string, Args][] =
    "onSuccessUpdateEmail" in raw ? [["Email/set", implicitSet(server, raw, sent)]] : [];
  return [["EmailSubmission/set", answer], ...implicit];
}
