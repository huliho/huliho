// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Longest address: the 256-octet path of RFC 5321 section 4.5.3.1.3
// minus its angle brackets.
const ADDRESS_MAX_BYTES = 254;
// Longest DNS label (RFC 1035 section 2.3.4).
const LABEL_MAX_BYTES = 63;
// A mail domain is a name under a top-level domain.
const DOMAIN_MIN_LABELS = 2;

const BLANK_OR_CONTROL = /[\p{White_Space}\p{Cc}]/u;
// Anything the URL parser would read as more than a host.
const NOT_A_HOST = /[\s:/?#@%[\]\\]/;
const LABEL = /^[a-z0-9-]+$/;
const ADDRESS_LITERAL = /^[\d.]+$/;
// One atom of a local part: the atext of RFC 5322 section 3.2.3.
const LOCAL_ATOM = /^[\w!#$%&'*+/=?^`{|}~-]+$/;

function fitsLabel(label: string): boolean {
  return (
    LABEL.test(label) &&
    label.length <= LABEL_MAX_BYTES &&
    !label.startsWith("-") &&
    !label.endsWith("-")
  );
}

// The labels of a host name in their ASCII lowercase form, as the
// server maps them; null for anything that is not a plain name.
function hostLabels(text: string): string[] | null {
  if (text === "" || NOT_A_HOST.test(text)) {
    return null;
  }
  let host: string;
  try {
    host = new URL(`https://${text}`).hostname;
  } catch {
    return null;
  }
  if (ADDRESS_LITERAL.test(host)) {
    return null;
  }
  const labels = host.split(".");
  return labels.every(fitsLabel) ? labels : null;
}

// A server name the server accepts as a target: a name, never an
// address literal.
export function fitsHostName(text: string): boolean {
  return hostLabels(text) !== null;
}

// A host name in its ASCII lowercase form, so two spellings of one
// name compare equal; null for anything that is not a plain name.
export function asciiHostName(text: string): string | null {
  return hostLabels(text)?.join(".") ?? null;
}

// A local part as a header carries it: a dot-atom of printable ASCII
// (RFC 5322 section 3.4.1) without the characters of an address list,
// so one address never reads as two.
function fitsLocalPart(local: string): boolean {
  return local.split(".").every((atom) => LOCAL_ATOM.test(atom));
}

// An address in the form a header carries it: a dot-atom local part in
// ASCII as typed and the domain as its ASCII lowercase name (RFC 5322
// section 2.2); null for anything a header cannot carry.
export function asciiAddress(address: string): string | null {
  if (BLANK_OR_CONTROL.test(address)) {
    return null;
  }
  const at = address.indexOf("@");
  if (at <= 0 || address.includes("@", at + 1)) {
    return null;
  }
  const local = address.slice(0, at);
  const labels = hostLabels(address.slice(at + 1));
  if (labels === null || labels.length < DOMAIN_MIN_LABELS || !fitsLocalPart(local)) {
    return null;
  }
  const ascii = `${local}@${labels.join(".")}`;
  return ascii.length > ADDRESS_MAX_BYTES ? null : ascii;
}

// Whether two spellings name one address: compared in the ASCII form,
// in any case.
export function sameAddress(one: string, other: string): boolean {
  return (asciiAddress(one) ?? one).toLowerCase() === (asciiAddress(other) ?? other).toLowerCase();
}

// Mirrors the server's reading of an address, so a typo never costs a
// round trip: bounded, one @ between two parts, no whitespace or
// control character and a domain of at least two plain labels.
export function fitsAddress(address: string): boolean {
  if (
    new TextEncoder().encode(address).length > ADDRESS_MAX_BYTES ||
    BLANK_OR_CONTROL.test(address)
  ) {
    return false;
  }
  const at = address.indexOf("@");
  if (at <= 0 || address.includes("@", at + 1)) {
    return false;
  }
  const labels = hostLabels(address.slice(at + 1));
  return labels !== null && labels.length >= DOMAIN_MIN_LABELS;
}
