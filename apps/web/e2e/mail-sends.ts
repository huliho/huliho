// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The sending side of the mocked account: its one identity (RFC 8621
// section 6), EmailSubmission/set with the implicit Email/set after it
// (section 7.5), a scripted refusal and a hold for the queue's tests,
// and the upload route that takes a file and answers its blob id (RFC
// 8620 section 6.1).

import type { BrowserContext, Route } from "@playwright/test";

import type { Invocation, MailServer } from "./mail-answers";
import { ACCOUNT_ADDRESS, UPSTREAM } from "./mail-corpus";
import { isRecord } from "./mail-folder-rules";
import { applyEmailSet } from "./mail-writes";

type Args = Record<string, unknown>;
type Response = [string, Args];

const UPLOAD_ROUTE = "**/api/jmap/*/upload/*";
const PAYLOAD_TOO_LARGE = 413;
// The type a body declares when it names none.
const OCTET_STREAM = "application/octet-stream";
// The bridge's limits: the largest upload and the attachments of one
// email, and how many uploads run at once.
export const MAX_UPLOAD_BYTES = 25 * 1024 * 1024;
export const MAX_CONCURRENT_UPLOADS = 2;
// The account's one identity, which never changes, so its state stands still.
const IDENTITY = {
  id: "identity-1",
  name: "",
  email: ACCOUNT_ADDRESS,
  replyTo: null,
  bcc: null,
  textSignature: "",
  htmlSignature: "",
  mayDelete: false,
};
const IDENTITY_STATE = "i1";
// The header fields a send reads its recipients from, in this order.
const RECIPIENT_FIELDS = ["to", "cc", "bcc"];

// The identity for ids null or absent, else the ids it is named by.
export function identityGet(_server: MailServer, args: Args): Args {
  const ids = Array.isArray(args["ids"]) ? args["ids"].map(String) : null;
  const shown = ids === null || ids.includes(IDENTITY.id);
  return {
    accountId: UPSTREAM,
    state: IDENTITY_STATE,
    list: shown ? [IDENTITY] : [],
    notFound: (ids ?? []).filter((id) => id !== IDENTITY.id),
  };
}

// The email id a submission names, a creation id of the request read
// through what it made (RFC 8620 section 5.3).
function emailIdOf(object: Args, made: ReadonlyMap<string, string>): string {
  const given = object["emailId"];
  const named = typeof given === "string" ? given : "";
  return named.startsWith("#") ? (made.get(named.slice(1)) ?? named) : named;
}

// The recipients of the saved email: every address of its to, cc and
// bcc once, compared without regard to case and spelled as first given.
function recipientsOf(server: MailServer, emailId: string): string[] {
  const saved = new Map(Object.entries(server.saved.get(emailId) ?? {}));
  const listed = RECIPIENT_FIELDS.flatMap((field) => {
    const list = saved.get(field);
    return Array.isArray(list) ? list.filter((address) => isRecord(address)) : [];
  });
  const once = new Map<string, string>();
  for (const address of listed) {
    const email = address["email"];
    if (typeof email === "string" && !once.has(email.toLowerCase())) {
      once.set(email.toLowerCase(), email);
    }
  }
  return [...once.values()];
}

// What the server refuses in a submission, before anything goes out.
function submissionError(server: MailServer, object: Args, emailId: string): Args | null {
  if (server.sendError !== null) {
    return { type: server.sendError };
  }
  if (object["identityId"] !== IDENTITY.id) {
    return { type: "invalidProperties", properties: ["identityId"] };
  }
  if (!server.corpus.emails.has(emailId)) {
    return { type: "invalidProperties", properties: ["emailId"] };
  }
  return recipientsOf(server, emailId).length > 0 ? null : { type: "noRecipients" };
}

// An address as an envelope names it (RFC 8621 section 7).
function envelopeAddress(email: string): Args {
  return { email, parameters: null };
}

// The envelope the server derives where a create names none: the
// identity as the sender, then the recipients of the saved email.
function envelopeOf(server: MailServer, emailId: string): Args {
  return {
    mailFrom: envelopeAddress(IDENTITY.email),
    rcptTo: recipientsOf(server, emailId).map((email) => envelopeAddress(email)),
  };
}

// What created answers of a submission that went out: every property
// the server sets (RFC 8620 section 5.3), the envelope among them only
// where the create gave none.
function createdObject(server: MailServer, object: Args, emailId: string, id: string): Args {
  const given = object["envelope"] !== undefined && object["envelope"] !== null;
  return {
    id,
    threadId: server.corpus.emails.get(emailId)?.threadId ?? null,
    ...(given ? {} : { envelope: envelopeOf(server, emailId) }),
    sendAt: new Date().toISOString(),
    undoStatus: "final",
    deliveryStatus: null,
    dsnBlobIds: [],
    mdnBlobIds: [],
  };
}

// The implicit Email/set: the patch of each submission that went out
// applied to its email and the emails named for destruction destroyed.
function implicitSet(server: MailServer, args: Args, sent: ReadonlyMap<string, string>): Args {
  const patches = isRecord(args["onSuccessUpdateEmail"]) ? args["onSuccessUpdateEmail"] : {};
  const update = Object.fromEntries(
    Object.entries(patches).flatMap(([key, patch]) => {
      const emailId = sent.get(key);
      return emailId === undefined ? [] : [[emailId, patch]];
    }),
  );
  const named = Array.isArray(args["onSuccessDestroyEmail"]) ? args["onSuccessDestroyEmail"] : [];
  const destroy = named.flatMap((key) => sent.get(String(key)) ?? []);
  return applyEmailSet(server, { accountId: UPSTREAM, update, destroy });
}

// EmailSubmission/set: each create the server takes goes out at once,
// final; then the implicit Email/set where the call asks for one.
export function emailSubmissionSet(
  server: MailServer,
  args: Args,
  made: Map<string, string>,
): Response[] {
  const oldState = String(server.submissions.length);
  const creates = isRecord(args["create"]) ? Object.entries(args["create"]) : [];
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
    made.set(key, id);
    sent.set(`#${key}`, emailId);
    created.push([key, createdObject(server, asked, emailId, id)]);
  }
  const answer = {
    accountId: UPSTREAM,
    oldState,
    newState: String(server.submissions.length),
    created: created.length > 0 ? Object.fromEntries(created) : null,
    notCreated: notCreated.length > 0 ? Object.fromEntries(notCreated) : null,
    updated: null,
    destroyed: null,
    notUpdated: null,
    notDestroyed: null,
  };
  const responses: Response[] = [["EmailSubmission/set", answer]];
  if ("onSuccessUpdateEmail" in args || "onSuccessDestroyEmail" in args) {
    responses.push(["Email/set", implicitSet(server, args, sent)]);
  }
  return responses;
}

// Whether a request sends a message.
export function asksSend(calls: readonly Invocation[]): boolean {
  return calls.some(([name]) => name === "EmailSubmission/set");
}

// Lets every held send through and holds no more.
export function releaseSends(server: MailServer): void {
  server.holdSends = false;
  for (const go of server.heldSends.splice(0)) {
    go();
  }
}

// A body within the scripted limit lands under a new blob id with the
// type it declares and its size; a larger one answers 413 as the route
// does.
function uploaded(server: MailServer, route: Route): Promise<void> {
  const request = route.request();
  if (request.method() !== "POST") {
    return route.fallback();
  }
  const size = request.postDataBuffer()?.length ?? 0;
  if (size > server.uploadLimit) {
    return route.fulfill({ status: PAYLOAD_TOO_LARGE, json: { error: "too_large" } });
  }
  const blobId = `upload-${String(server.uploads.size + 1)}`;
  const type = request.headers()["content-type"] ?? OCTET_STREAM;
  server.uploads.set(blobId, { type, size });
  const accountId = new URL(request.url()).pathname.split("/").at(-1) ?? UPSTREAM;
  return route.fulfill({ json: { accountId, blobId, type, size } });
}

// Answers the upload route of any account from the server's store.
export async function mockUploads(context: BrowserContext, server: MailServer): Promise<void> {
  await context.route(UPLOAD_ROUTE, (route) => uploaded(server, route));
}
