// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { tokenize } from "./addresses";
import type { Recipient } from "./addresses";
import { messageIds } from "./headers";

// The longest `mailto:` URI the composer opens; anything longer opens
// nothing, so a link cannot hand it a message of any size.
export const MAILTO_MAX_CHARS = 8192;

const SCHEME = "mailto:";
const LINE_BREAKS = /\r\n|\r/gu;
const HEADER_BREAKS = /[\r\n\0]+/gu;
const ANGLED_ID = /<([^<>]*)>/gu;

// What a `mailto:` link fills a new draft with.
export interface MailtoDraft {
  to: Recipient[];
  cc: Recipient[];
  bcc: Recipient[];
  subject: string;
  body: string;
  inReplyTo: string[];
}

function decoded(text: string): string | null {
  try {
    return decodeURIComponent(text);
  } catch {
    return null;
  }
}

// The fields of the URI by name, each value percent-decoded; a field
// that does not decode is left out.
function fieldsOf(path: string, query: string): Map<string, string[]> {
  const fields = new Map<string, string[]>([["to", [decoded(path) ?? ""]]]);
  for (const pair of query.split("&")) {
    const at = pair.indexOf("=");
    const name = decoded(at < 0 ? pair : pair.slice(0, at))?.toLowerCase();
    const value = decoded(at < 0 ? "" : pair.slice(at + 1));
    if (name !== undefined && value !== null) {
      fields.set(name, [...(fields.get(name) ?? []), value]);
    }
  }
  return fields;
}

// A `mailto:` URI as a draft for the user to see first (RFC 6068): to, cc,
// bcc, subject, body and in-reply-to, the fields section 3 names safe,
// a repeated field joined and the body's line breaks as line feeds.
// Every other field is ignored, as section 7 asks of originator, routing,
// trace and MIME fields. Null for anything else or anything past
// MAILTO_MAX_CHARS.
export function readMailto(uri: string): MailtoDraft | null {
  if (uri.length > MAILTO_MAX_CHARS || uri.slice(0, SCHEME.length).toLowerCase() !== SCHEME) {
    return null;
  }
  const rest = uri.slice(SCHEME.length);
  const mark = rest.indexOf("?");
  const fields = fieldsOf(
    mark < 0 ? rest : rest.slice(0, mark),
    mark < 0 ? "" : rest.slice(mark + 1),
  );
  const joined = (name: string, between: string): string => (fields.get(name) ?? []).join(between);
  const replied = [...joined("in-reply-to", " ").matchAll(ANGLED_ID)].map(([, id]) => id ?? "");
  return {
    to: tokenize(joined("to", ",")),
    cc: tokenize(joined("cc", ",")),
    bcc: tokenize(joined("bcc", ",")),
    subject: joined("subject", " ").replaceAll(HEADER_BREAKS, " ").trim(),
    body: joined("body", "\n").replaceAll(LINE_BREAKS, "\n"),
    inReplyTo: messageIds(replied),
  };
}
