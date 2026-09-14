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
