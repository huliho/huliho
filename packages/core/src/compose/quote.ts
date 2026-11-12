// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DraftAttachment, EmailBody } from "../cache/store";
import type { EmailBodyPart } from "../jmap/body";
import type { EmailHeader } from "../jmap/schemas";
import { quotedLines, unflow } from "../text/flowed";
import type { QuotedLine } from "../text/flowed";

// The words of a forward's header in the reader's language.
export interface ForwardWords {
  title: string;
  from: string;
  date: string;
  to: string;
  subject: string;
}

// The rule around a forward's title, as mail clients write it.
const FORWARD_RULE = "--------";

// Unicode's first strong isolate and its end (UAX 9): the name takes its
// own direction inside the sentence, in the HTML and the text alike.
const ISOLATE_START = String.fromCodePoint(0x20_68);
const ISOLATE_END = String.fromCodePoint(0x20_69);

const HTML_ESCAPES: ReadonlyMap<string, string> = new Map([
  ["&", "&amp;"],
  ["<", "&lt;"],
  [">", "&gt;"],
  ['"', "&quot;"],
  ["'", "&#39;"],
]);

function escaped(text: string): string {
  return text.replaceAll(/[&<>"']/gu, (character) => HTML_ESCAPES.get(character) ?? character);
}

function isolated(text: string): string {
  return `${ISOLATE_START}${text}${ISOLATE_END}`;
}

// Who wrote the parent and when, for the line over a quote. The date is
// when it was sent, else when it arrived. The name is the sender's, else
// the address, isolated so its direction holds in the sentence.
export function attributionOf(parent: Pick<EmailHeader, "from" | "sentAt" | "receivedAt">): {
  date: string;
  name: string;
} {
  const [sender] = parent.from ?? [];
  const name = sender === undefined ? "" : sender.name?.trim() || sender.email;
  return { date: parent.sentAt ?? parent.receivedAt, name: isolated(name) };
}

// A reply's quote as the editor takes it in: the attribution over the
// parent's sanitized HTML in a cite blockquote, its pictures keeping
// their `cid:` addresses (RFC 2392).
export function replyHtml(attribution: string, html: string): string {
  return `<p>${escaped(attribution)}</p><blockquote type="cite">${html}</blockquote>`;
}

// The paragraphs of the parent's text part with their quote depth, read
// through format=flowed where the part carries it; null where the
// parent has no text part to read.
function parentLines(body: EmailBody): QuotedLine[] | null {
  const plain = body.textBody.filter((part) => part.type === "text/plain");
  if (plain.length === 0) {
    return null;
  }
  const text = plain
    .map((part) => (part.partId === null ? "" : (body.bodyValues[part.partId]?.value ?? "")))
    .join("\n");
  return body.flowed === null ? quotedLines(text) : unflow(text, body.flowed.delSp);
}

// A reply's text quote: the attribution, then the parent's paragraphs one
// depth deeper than they stood; null where the parent has no text part,
// so the composer quotes its HTML instead.
export function replyLines(attribution: string, body: EmailBody): QuotedLine[] | null {
  const lines = parentLines(body);
  return lines === null
    ? null
    : [
        { depth: 0, text: attribution },
        ...lines.map(({ depth, text }) => ({ depth: depth + 1, text })),
      ];
}

// What a forward's header names of the parent, each value as the reader
// sees it.
export interface ForwardFacts {
  from: string;
  date: string;
  to: string;
  subject: string;
}

function headerLines(words: ForwardWords, facts: ForwardFacts): string[] {
  return [
    `${FORWARD_RULE} ${words.title} ${FORWARD_RULE}`,
    `${words.from}: ${facts.from}`,
    `${words.date}: ${facts.date}`,
    `${words.to}: ${facts.to}`,
    `${words.subject}: ${facts.subject}`,
  ];
}

// A forward's HTML as the editor takes it in: the header, then the
// parent's sanitized HTML as it stood.
export function forwardHtml(words: ForwardWords, facts: ForwardFacts, html: string): string {
  return `<p>${headerLines(words, facts).map(escaped).join("<br>")}</p>${html}`;
}

// A forward's text: the header, then the parent's paragraphs at their own
// depth; null where the parent has no text part.
export function forwardLines(
  words: ForwardWords,
  facts: ForwardFacts,
  body: EmailBody,
): QuotedLine[] | null {
  const lines = parentLines(body);
  return lines === null
    ? null
    : [...headerLines(words, facts).map((text) => ({ depth: 0, text })), ...lines];
}

function fileOf(
  part: EmailBodyPart,
  disposition: DraftAttachment["disposition"],
): DraftAttachment[] {
  return part.blobId === null
    ? []
    : [
        {
          blobId: part.blobId,
          name: part.name,
          type: part.type,
          size: part.size,
          cid: disposition === "inline" ? part.cid : null,
          disposition,
        },
      ];
}

// A `cid:` reference up to the quote, the space or the bracket that ends it.
const CID_REFERENCE = /cid:([^"'\s)>]+)/giu;

// The ids the HTML names by `cid:`, percent-decoded as the URL carries
// them (RFC 2392 section 2); a reference that does not decode stays as
// written.
function namedByHtml(html: string): Set<string> {
  return new Set(
    [...html.matchAll(CID_REFERENCE)].map(([, reference = ""]) => {
      try {
        return decodeURIComponent(reference);
      } catch {
        return reference;
      }
    }),
  );
}

// The parent's pictures its HTML names by `cid:`, carried by their blobs
// as inline parts of the answer (RFC 2392), each once.
export function quotedParts(body: EmailBody, html: string): DraftAttachment[] {
  const named = namedByHtml(html);
  const files = [...body.htmlBody, ...body.attachments]
    .filter((part) => part.cid !== null && named.has(part.cid))
    .flatMap((part) => fileOf(part, "inline"));
  return files.filter(
    (file, index) => files.findIndex((one) => one.blobId === file.blobId) === index,
  );
}

// What a forward carries: the pictures its HTML names as inline parts and
// every other attachment of the parent as an attachment.
export function forwardedParts(body: EmailBody, html: string): DraftAttachment[] {
  const inline = quotedParts(body, html);
  const carried = new Set(inline.map((file) => file.blobId));
  const attached = body.attachments
    .filter((part) => part.blobId !== null && !carried.has(part.blobId))
    .flatMap((part) => fileOf(part, "attachment"));
  return [...inline, ...attached];
}
