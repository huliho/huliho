// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "@huliho/core";

export interface Address {
  name: string | null;
  email: string;
}

// Who wrote a message: the name as shown and the address beside it,
// which stays out when it is the name.
export interface Sender {
  name: string | null;
  address: string | null;
}

// The recipients of a message by field, in the order they are read out.
export type RecipientField = "to" | "cc" | "bcc" | "replyTo";

export const RECIPIENT_FIELDS: readonly RecipientField[] = ["to", "cc", "bcc", "replyTo"];

// One list formatter per locale: building one costs more than a card may spend.
const formatters = new Map<string, Intl.ListFormat>();

function formatter(locale: string): Intl.ListFormat {
  const held = formatters.get(locale);
  if (held !== undefined) {
    return held;
  }
  const made = new Intl.ListFormat(locale, { type: "conjunction" });
  formatters.set(locale, made);
  return made;
}

// The name an address is shown by: its display name, else the address.
export function displayName(address: Address): string {
  return address.name === null || address.name.trim() === "" ? address.email : address.name;
}

export function senderOf(email: EmailHeader): Sender {
  const address = email.from?.[0] ?? email.sender?.[0];
  if (address === undefined) {
    return { name: null, address: null };
  }
  const name = displayName(address);
  return { name, address: name === address.email ? null : address.email };
}

// The names of a field's recipients as one phrase in the locale; empty
// when the field names nobody.
export function recipientNames(addresses: readonly Address[] | null, locale: string): string {
  if (addresses === null || addresses.length === 0) {
    return "";
  }
  return formatter(locale).format(addresses.map(displayName));
}

function fieldOf(email: EmailHeader, field: RecipientField): readonly Address[] | null {
  switch (field) {
    case "to":
      return email.to;
    case "cc":
      return email.cc;
    case "bcc":
      return email.bcc;
    default:
      return email.replyTo;
  }
}

// The addresses under a field, each once, none for a field the message
// does not carry.
export function recipientsIn(email: EmailHeader, field: RecipientField): Address[] {
  const seen = new Set<string>();
  return (fieldOf(email, field) ?? []).filter((address) => {
    if (seen.has(address.email)) {
      return false;
    }
    seen.add(address.email);
    return true;
  });
}
