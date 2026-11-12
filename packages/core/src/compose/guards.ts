// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DraftContent } from "../cache/store";
import { unfitText } from "./addresses";

// From this many recipients on, the composer says everyone sees the list.
export const LARGE_RECIPIENT_LIST = 10;

// A sentence above Send that a key dismisses and that blocks nothing: a
// word of attaching with nothing attached, an empty subject or many
// recipients who see each other.
export type Guardrail =
  | { kind: "attachment"; word: string }
  | { kind: "subject" }
  | { kind: "recipients"; count: number };

// What refuses Send in place: text that is no address, no recipient at
// all or attachments past what the account takes for one email.
export type Refusal =
  { kind: "notAnAddress"; text: string } | { kind: "noRecipient" } | { kind: "tooLarge" };

// A letter or a digit beside a match makes it part of another word.
const WORD_CHARACTER = /[\p{L}\p{N}]/u;

// Whether the word stands in the text as a whole word, in any case.
function standsAlone(text: string, word: string): boolean {
  const haystack = text.toLowerCase();
  const needle = word.toLowerCase();
  for (let at = haystack.indexOf(needle); at >= 0; at = haystack.indexOf(needle, at + 1)) {
    const before = haystack.slice(Math.max(0, at - 1), at);
    const after = haystack.slice(at + needle.length, at + needle.length + 1);
    if (!WORD_CHARACTER.test(before) && !WORD_CHARACTER.test(after)) {
      return true;
    }
  }
  return false;
}

function recipientsOf(content: DraftContent): number {
  return content.to.length + content.cc.length + content.bcc.length;
}

// The first of the locale's words of attaching that stands as a whole
// word in the user's own text; the caller leaves the quote out.
export function attachingWord(text: string, words: readonly string[]): string | null {
  return words.find((word) => word.trim() !== "" && standsAlone(text, word.trim())) ?? null;
}

// The one guardrail the draft meets that the user did not dismiss: the
// attachment one first, then the subject, then the recipient count.
export function guardrailOf(
  content: DraftContent,
  text: string,
  words: readonly string[],
  dismissed: ReadonlySet<Guardrail["kind"]>,
): Guardrail | null {
  const attached = content.attachments.some((file) => file.disposition === "attachment");
  const word = attached || dismissed.has("attachment") ? null : attachingWord(text, words);
  const count = recipientsOf(content);
  const met: Guardrail[] = [
    ...(word === null ? [] : [{ kind: "attachment" as const, word }]),
    ...(content.subject.trim() === "" ? [{ kind: "subject" as const }] : []),
    ...(count >= LARGE_RECIPIENT_LIST ? [{ kind: "recipients" as const, count }] : []),
  ];
  return met.find((guardrail) => !dismissed.has(guardrail.kind)) ?? null;
}

// What refuses Send in place, the first that holds: a token no header
// carries, text or an address that fails `asciiAddress`, then no
// recipient, then attachments past the account's
// `maxSizeAttachmentsPerEmail`, null where the account names none.
export function refusalOf(content: DraftContent, maxAttachments: number | null): Refusal | null {
  const tokens = [...content.to, ...content.cc, ...content.bcc];
  const text = tokens.map(unfitText).find((unfit) => unfit !== null);
  if (text !== undefined) {
    return { kind: "notAnAddress", text };
  }
  if (tokens.length === 0) {
    return { kind: "noRecipient" };
  }
  const weight = content.attachments.reduce((total, file) => total + file.size, 0);
  return maxAttachments !== null && weight > maxAttachments ? { kind: "tooLarge" } : null;
}
