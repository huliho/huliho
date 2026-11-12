// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { addressParser } from "postal-mime";
import type { Address } from "postal-mime";

import { asciiAddress } from "../address";
import type { MailAddress } from "./headers";

// One token of a recipient field: an address a header carries with the
// name beside it, or text the user wrote that is no such address. Such
// text is kept so the user can mend it.
export type Recipient = MailAddress | { text: string };

// Line breaks separate the entries of a pasted list as commas do.
const LINE_BREAKS = /\r\n|[\r\n]/gu;
// A name never carries a control character into a header.
const CONTROL = /\p{Cc}+/gu;

export function isAddress(recipient: Recipient): recipient is MailAddress {
  return "email" in recipient;
}

function nameOf(name: string): string | null {
  const plain = name.replaceAll(CONTROL, " ").trim();
  return plain === "" ? null : plain;
}

// The text of an entry that is no address a header carries: its name and
// its address as written, either one alone where the other is missing.
function textOf(name: string, email: string): string {
  const shown = nameOf(name);
  return shown === null || email === "" ? (shown ?? email) : `${shown} <${email}>`;
}

function tokenOf({ name, address }: { name: string; address: string }): Recipient[] {
  const email = address.trim();
  if (email === "" && name.trim() === "") {
    return [];
  }
  if (asciiAddress(email) !== null) {
    return [{ name: nameOf(name), email }];
  }
  return [{ text: textOf(name, email) }];
}

// The token of an address a message carries: the address itself where a
// header carries it (`asciiAddress`), else the text a typed entry gives.
export function addressToken(address: MailAddress): Recipient[] {
  return asciiAddress(address.email) === null
    ? tokenOf({ name: address.name ?? "", address: address.email })
    : [address];
}

// The text of a token no header carries: a text token's own or an address
// that fails `asciiAddress`, written as text; null for any other address.
export function unfitText(recipient: Recipient): string | null {
  if (!isAddress(recipient)) {
    return recipient.text;
  }
  return asciiAddress(recipient.email) === null
    ? textOf(recipient.name ?? "", recipient.email.trim())
    : null;
}

function tokensOf(entry: Address): Recipient[] {
  return entry.group === undefined ? tokenOf(entry) : entry.group.flatMap(tokenOf);
}

// The tokens of a pasted or typed list (RFC 5322 section 3.4): one per
// address, a quoted name with a comma in it kept whole and a group's
// members as tokens of their own. An entry that is no address a header
// carries (`asciiAddress`) comes back as text.
export function tokenize(text: string): Recipient[] {
  return addressParser(text.replaceAll(LINE_BREAKS, ",")).flatMap(tokensOf);
}

function keyOf(recipient: Recipient): string {
  return isAddress(recipient)
    ? (asciiAddress(recipient.email) ?? recipient.email).toLowerCase()
    : `\0${recipient.text}`;
}

// The tokens of a field with the new ones after them; an address or a
// text the field holds already is left out.
export function withTokens(held: readonly Recipient[], added: readonly Recipient[]): Recipient[] {
  const seen = new Set(held.map(keyOf));
  const kept = [...held];
  for (const token of added) {
    const key = keyOf(token);
    if (!seen.has(key)) {
      seen.add(key);
      kept.push(token);
    }
  }
  return kept;
}
