// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "../jmap/schemas";
import { SENDER_SUGGESTIONS } from "./limits";
import type { MailStore, Sender } from "./store";

// The marks a letter carries once it is taken apart, so "zoe" finds Zoë.
const MARKS = /\p{M}/gu;
// The first letter or digit of each word.
const WORD_START = /(?<![\p{L}\p{N}])[\p{L}\p{N}]/gu;

// One sender as the headers are read, with when its address and its name
// were last seen.
interface Tally {
  sender: Sender;
  newestAt: number;
  namedAt: number;
}

// Every address the headers name in `from`, one sender per address in
// any case: the spelling and the time of the newest header, the name of
// the newest header that gives one.
export function sendersOf(headers: Iterable<Pick<EmailHeader, "from" | "receivedAt">>): Sender[] {
  const found = new Map<string, Tally>();
  for (const header of headers) {
    const at = Date.parse(header.receivedAt);
    for (const address of header.from ?? []) {
      const key = address.email.toLowerCase();
      const tally = found.get(key) ?? {
        sender: { email: address.email, name: null, count: 0, newest: header.receivedAt },
        newestAt: Number.NEGATIVE_INFINITY,
        namedAt: Number.NEGATIVE_INFINITY,
      };
      tally.sender.count += 1;
      if (at >= tally.newestAt) {
        tally.newestAt = at;
        tally.sender.email = address.email;
        tally.sender.newest = header.receivedAt;
      }
      if (address.name !== null && address.name !== "" && at >= tally.namedAt) {
        tally.namedAt = at;
        tally.sender.name = address.name;
      }
      found.set(key, tally);
    }
  }
  return [...found.values()].map((tally) => tally.sender);
}

function folded(text: string): string {
  return text.normalize("NFD").replace(MARKS, "").toLowerCase();
}

// Whether the name or the address, from its start or from the start of
// any word in it, begins with the typed prefix in any case and without
// its marks.
function matches(sender: Sender, prefix: string): boolean {
  return [sender.name ?? "", sender.email].map(folded).some((part) => {
    const starts = [0, ...[...part.matchAll(WORD_START)].map((match) => match.index)];
    return starts.some((start) => part.startsWith(prefix, start));
  });
}

// The senders a suggestion list offers for what the user typed: the ones
// that match, most mail first, then the newest; an empty prefix matches
// every sender.
export async function readSenders(
  store: MailStore,
  accountId: string,
  prefix: string,
): Promise<Sender[]> {
  const wanted = folded(prefix.trim());
  const senders = await store.senders(accountId);
  return senders
    .filter((sender) => matches(sender, wanted))
    .toSorted(
      (a, b) =>
        b.count - a.count ||
        Date.parse(b.newest) - Date.parse(a.newest) ||
        a.email.localeCompare(b.email),
    )
    .slice(0, SENDER_SUGGESTIONS);
}
