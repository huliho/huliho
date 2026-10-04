// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { asciiHostName } from "./address";
import type { Authentication } from "./auth-results";
import { CSRF_HEADERS } from "./http";
import { z } from "./schema";

const SENDER_POLICIES_ENDPOINT = "/api/sender-policies";

// The one policy a sender carries: remote content may load.
const REMOTE_CONTENT = "remoteContent";

// The longest address a policy is keyed on, as the server bounds it.
const SENDER_MAX_BYTES = 320;

const CONTROL = /\p{Cc}/u;

// The dot that closes a fully qualified name; a header's domain comes
// without it.
const CLOSING_DOT = /\.$/;

// A grant with the receiving server it was given under: the authserv-id
// of the message the reader allowed, the empty string for a header that
// named none and null for a message without a header.
const grantSchema = z.object({ allow: z.literal(true), authserv: z.string().nullable() });

const senderPolicySchema = z.object({
  sender: z.string(),
  key: z.literal(REMOTE_CONTENT),
  value: grantSchema,
});

export type RemoteContentGrant = z.infer<typeof grantSchema>;
export type SenderPolicy = z.infer<typeof senderPolicySchema>;

export type SenderPoliciesFailureCode = "unauthenticated" | "unavailable";

export class SenderPoliciesError extends Error {
  readonly code: SenderPoliciesFailureCode;

  constructor(code: SenderPoliciesFailureCode) {
    super(`sender policies request failed: ${code}`);
    this.name = "SenderPoliciesError";
    this.code = code;
  }
}

// What a policy is keyed on: the first address of From, lowercased as
// the server stores it. Null for a message without one the server would
// take, which then has no policy.
export function senderKey(from: readonly { email: string }[] | null): string | null {
  const address = from?.at(0)?.email.toLowerCase();
  if (address === undefined || CONTROL.test(address)) {
    return null;
  }
  const at = address.lastIndexOf("@");
  const fits = new TextEncoder().encode(address).length <= SENDER_MAX_BYTES;
  return at > 0 && at < address.length - 1 && fits ? address : null;
}

// Whether a grant lets a message load remote content: only while the
// receiving server says what it said at the grant, since From is the
// sender's own word.
export function grantLoads(
  grant: RemoteContentGrant,
  sender: string,
  authentication: Authentication,
): boolean {
  if (authentication.status !== "parsed") {
    return authentication.status === "absent" && grant.authserv === null;
  }
  const { server, dmarc, dmarcFrom } = authentication.results;
  const written = sender.slice(sender.lastIndexOf("@") + 1).replace(CLOSING_DOT, "");
  const domain = asciiHostName(written);
  const checked = asciiHostName(dmarcFrom ?? "");
  return (
    (server ?? "") === grant.authserv && dmarc === "pass" && checked === domain && domain !== null
  );
}

// The grant a reader can give on a message, pinned to what its topmost
// header says; null when the message would not load under its own grant.
export function grantFor(
  sender: string,
  authentication: Authentication,
): RemoteContentGrant | null {
  const pin = authentication.status === "parsed" ? (authentication.results.server ?? "") : null;
  const grant: RemoteContentGrant = { allow: true, authserv: pin };
  return grantLoads(grant, sender, authentication) ? grant : null;
}

export async function fetchSenderPolicies(): Promise<SenderPolicy[]> {
  const response = await fetch(SENDER_POLICIES_ENDPOINT);
  if (!response.ok) {
    throw new Error(`the sender policies request failed with status ${String(response.status)}`);
  }
  return z.array(senderPolicySchema).parse(await response.json());
}

// A session that ended is the one refusal the caller acts on; every
// other refusal reads as unavailable.
async function write(path: string, init: RequestInit): Promise<void> {
  let response: Response;
  try {
    response = await fetch(`${SENDER_POLICIES_ENDPOINT}/${path}`, init);
  } catch {
    throw new SenderPoliciesError("unavailable");
  }
  if (!response.ok) {
    throw new SenderPoliciesError(response.status === 401 ? "unauthenticated" : "unavailable");
  }
}

// Remote content loads for the sender from now on, under the grant's pin.
export function allowRemoteContent(sender: string, grant: RemoteContentGrant): Promise<void> {
  return write(encodeURIComponent(sender), {
    method: "PUT",
    headers: { "content-type": "application/json", ...CSRF_HEADERS },
    body: JSON.stringify({ key: REMOTE_CONTENT, value: grant }),
  });
}

export function blockRemoteContent(sender: string): Promise<void> {
  return write(`${encodeURIComponent(sender)}/${REMOTE_CONTENT}`, {
    method: "DELETE",
    headers: CSRF_HEADERS,
  });
}
