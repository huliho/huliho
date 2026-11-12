// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { sameAddress } from "../address";
import type { EmailHeader } from "../jmap/schemas";
import { addressToken } from "./addresses";
import type { Recipient } from "./addresses";

// The one address of an email field, a name beside it or none.
export interface MailAddress {
  name: string | null;
  email: string;
}

// The ids a field may carry at most, as the bridge keeps them; a longer
// References keeps its first id and the newest ones.
export const MESSAGE_IDS_MAX = 256;

// The bytes of one id the bridge takes: one header line holds it with
// room to spare (RFC 5322 section 2.1.1).
export const MESSAGE_ID_BYTES = 512;

// An id without its angle brackets: printable ASCII with no space or
// angle bracket (RFC 5322 section 3.6.4).
const MESSAGE_ID = /^[!-;=?-~]+$/u;

// What a reply or a forward is built from.
export type Answer = "reply" | "replyAll" | "forward";

// Whether the text is an id the bridge takes, a Message-ID and a
// Content-ID alike.
export function fitsId(id: string): boolean {
  return id.length <= MESSAGE_ID_BYTES && MESSAGE_ID.test(id);
}

// The ids of a field that a header can carry, the field kept to its
// first id and the newest ones past MESSAGE_IDS_MAX.
export function messageIds(ids: readonly string[]): string[] {
  const fit = ids.filter(fitsId);
  if (fit.length <= MESSAGE_IDS_MAX) {
    return fit;
  }
  return [...fit.slice(0, 1), ...fit.slice(fit.length - MESSAGE_IDS_MAX + 1)];
}

// In-Reply-To and References of a reply (RFC 5322 section 3.6.4): the
// parent's Message-ID, after the parent's References, else after its
// In-Reply-To where that names one id alone. A forward carries neither.
export function replyHeaders(
  parent: Pick<EmailHeader, "messageId" | "inReplyTo" | "references">,
  answer: Answer,
): { inReplyTo: string[]; references: string[] } {
  if (answer === "forward") {
    return { inReplyTo: [], references: [] };
  }
  const own = parent.messageId ?? [];
  const before = parent.references ?? (parent.inReplyTo?.length === 1 ? parent.inReplyTo : []);
  return { inReplyTo: messageIds(own), references: messageIds([...before, ...own]) };
}

// The literal prefixes of a reply and a forward, never localized (RFC
// 5322 section 3.6.5).
const REPLY = "Re: ";
const FORWARD = "Fwd: ";

// A reply or forward prefix (RFC 5256 section 2.1, subj-refwd) in any
// case, read in steps: the word, a count or a blob in brackets, then the
// colon.
const LEADER = /^(?:re|fwd?)[ \t]*/iu;
const COLON = /^:[ \t]*/u;
// A bracketed tag, which stays (subj-blob).
const TAG = /^\[[^\]]*\][ \t]*/u;

// How long the reply or forward prefix the text opens with is, 0 for none.
function prefixOf(text: string): number {
  const leader = LEADER.exec(text);
  if (leader === null) {
    return 0;
  }
  const count = TAG.exec(text.slice(leader[0].length));
  const at = leader[0].length + (count?.[0].length ?? 0);
  const colon = COLON.exec(text.slice(at));
  return colon === null ? 0 : at + colon[0].length;
}

// The subject without its leading reply and forward prefixes; a tag in
// brackets keeps its place in front.
function baseOf(subject: string): string {
  let rest = subject.trim();
  let tags = "";
  for (;;) {
    const prefix = prefixOf(rest);
    const tag = prefix === 0 ? TAG.exec(rest) : null;
    if (prefix === 0 && tag === null) {
      return `${tags}${rest}`;
    }
    rest = rest.slice(prefix === 0 ? (tag?.[0].length ?? 0) : prefix);
    tags += tag === null ? "" : `${tag[0].trimEnd()} `;
  }
}

// The subject of a reply ("Re: ") or a forward ("Fwd: ") over the
// parent's, with its prefix written once.
export function answerSubject(subject: string | null, answer: Answer): string {
  const base = baseOf(subject ?? "");
  return `${answer === "forward" ? FORWARD : REPLY}${base}`.trimEnd();
}

// The addresses of the fields in order, each once, the addresses `left`
// names left out.
function onceWithout(
  fields: readonly (readonly MailAddress[])[],
  left: readonly string[],
): MailAddress[] {
  const kept: MailAddress[] = [];
  for (const address of fields.flat()) {
    const taken = [...left, ...kept.map((held) => held.email)];
    if (!taken.some((email) => sameAddress(email, address.email))) {
      kept.push({ name: address.name, email: address.email });
    }
  }
  return kept;
}

// To and Cc of a reply. The parent's Reply-To answers it, else its From;
// a message the account sent answers its own To. Reply all adds the
// parent's To and keeps its Cc, the account's own address left out. The
// parent's Sender and Bcc are no recipients. An address a header cannot
// carry comes back as text, so the field shows it and Send names it.
export function replyRecipients(
  parent: Pick<EmailHeader, "from" | "replyTo" | "to" | "cc">,
  answer: Exclude<Answer, "forward">,
  own: string,
): { to: Recipient[]; cc: Recipient[] } {
  const mine = (parent.from ?? []).some((address) => sameAddress(address.email, own));
  const parentTo = parent.to ?? [];
  const answering = mine ? parentTo : (parent.replyTo ?? parent.from ?? []);
  if (answer === "reply") {
    return { to: onceWithout([answering], []).flatMap(addressToken), cc: [] };
  }
  const to = onceWithout([answering, parentTo], [own]);
  const cc = onceWithout([parent.cc ?? []], [own, ...to.map((address) => address.email)]);
  return { to: to.flatMap(addressToken), cc: cc.flatMap(addressToken) };
}
