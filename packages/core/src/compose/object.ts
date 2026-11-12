// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { asciiAddress } from "../address";
import type { DraftAttachment, DraftContent } from "../cache/store";
import { unflow } from "../text/flowed";
import { isAddress } from "./addresses";
import type { Recipient } from "./addresses";
import { writeFixed } from "./flowed-write";
import { fitsId, messageIds } from "./headers";
import type { MailAddress } from "./headers";

// The part ids of the two text parts in `bodyValues`.
export const TEXT_PART = "text";
export const HTML_PART = "html";

// The flowed parameters in the raw form of the header (RFC 8621 section
// 4.1.2.1), which `type` has no room for; the server writes the charset.
const FLOWED_TYPE = " text/plain; format=flowed; delsp=yes";

// What a header and a value never carry: CR and LF end a field and a
// NUL fits no message.
const HEADER_BREAKS = /[\r\n\0]+/gu;
const NUL = /\0/gu;

// A media type as the bridge takes it: two tokens around one slash (RFC
// 2045 section 5.1).
const MEDIA_TYPE = /^([\w!#$%&'*+.^`|~-]+)\/([\w!#$%&'*+.^`|~-]+)$/;
// The bytes of a type or a subtype at most (RFC 6838 section 4.2).
const TOKEN_MAX_BYTES = 127;
// The type of a file that names none a header can carry (RFC 2046
// section 4.5.1).
const OCTET_STREAM = "application/octet-stream";
// The family of types that holds parts and never a file's bytes (RFC 2046
// section 5.1).
const MULTIPART = "multipart/";

// Where the draft is filed and whether the server writes a text part's
// flowed parameters with its charset.
export interface CreateContext {
  draftsId: string;
  flowedText: boolean;
}

// One part of a create's tree (RFC 8621 section 4.6).
export interface CreatePart {
  partId?: string;
  blobId?: string;
  type?: string;
  "header:Content-Type"?: string;
  name?: string;
  disposition?: string;
  cid?: string;
  subParts?: CreatePart[];
}

function headerText(text: string): string {
  return text.replaceAll(HEADER_BREAKS, " ");
}

// An address in the form a header carries: the domain as its ASCII name.
function headerAddress({ name, email }: MailAddress): MailAddress | null {
  const ascii = asciiAddress(email);
  const plain = name === null ? "" : headerText(name).trim();
  return ascii === null ? null : { name: plain === "" ? null : plain, email: ascii };
}

// The addresses of a field; text that is no address stays in the draft
// and out of the message.
function addressesOf(recipients: readonly Recipient[]): MailAddress[] {
  return recipients.filter(isAddress).flatMap((one) => headerAddress(one) ?? []);
}

// The type a create sends for a file: the file's own in lower case where
// it reads as two tokens and is no multipart, else the type of plain bytes.
export function sentType(file: Pick<DraftAttachment, "type">): string {
  const tokens = MEDIA_TYPE.exec(file.type)?.slice(1) ?? [];
  const fits = tokens.length > 0 && tokens.every((token) => token.length <= TOKEN_MAX_BYTES);
  const type = file.type.toLowerCase();
  return fits && !type.startsWith(MULTIPART) ? type : OCTET_STREAM;
}

function blobPart(file: DraftAttachment): CreatePart {
  return {
    blobId: file.blobId,
    type: sentType(file),
    disposition: file.disposition,
    ...(file.name === null ? {} : { name: headerText(file.name) }),
    ...(file.cid !== null && fitsId(file.cid) ? { cid: file.cid } : {}),
  };
}

function textPart(flowedText: boolean): CreatePart {
  return flowedText
    ? { partId: TEXT_PART, "header:Content-Type": FLOWED_TYPE }
    : { partId: TEXT_PART, type: "text/plain" };
}

// The tree of a create: multipart/mixed over the attachments, then
// multipart/related over the inline parts the HTML names, then
// multipart/alternative with the text and the HTML, each level only where
// it holds something. A plain draft has no HTML, so no related level.
export function treeOf(content: DraftContent, flowedText: boolean): CreatePart {
  const rich = content.mode === "rich";
  const text = textPart(flowedText);
  const alternative: CreatePart = rich
    ? { type: "multipart/alternative", subParts: [text, { partId: HTML_PART, type: "text/html" }] }
    : text;
  const { inline, attached } = filesOf(content);
  const related: CreatePart =
    inline.length > 0
      ? { type: "multipart/related", subParts: [alternative, ...inline.map(blobPart)] }
      : alternative;
  return attached.length > 0
    ? { type: "multipart/mixed", subParts: [related, ...attached.map(blobPart)] }
    : related;
}

// A part the HTML names by a `cid` a header can carry.
function named(file: DraftAttachment): boolean {
  return file.disposition === "inline" && file.cid !== null && fitsId(file.cid);
}

// The files the tree carries: the inline parts a rich draft's HTML names
// and every other file as it stands. A plain draft has no HTML to name a
// part, so each of its files goes at the mixed level in the row's order.
function filesOf(content: DraftContent): {
  inline: DraftAttachment[];
  attached: DraftAttachment[];
} {
  if (content.mode === "plain") {
    return { inline: [], attached: [...content.attachments] };
  }
  return {
    inline: content.attachments.filter(named),
    attached: content.attachments.filter((file) => !named(file)),
  };
}

// The files a create carries by blob, in the order the tree's leaves
// stand after its text parts.
export function carriedFiles(content: DraftContent): DraftAttachment[] {
  const { inline, attached } = filesOf(content);
  return [...inline, ...attached];
}

// How many text leaves open the tree: the text, and the HTML of a rich draft.
export function textLeaves(content: DraftContent): number {
  return content.mode === "rich" ? 2 : 1;
}

// The text part's value: as the composer wrote it where the server takes
// the flowed parameters, else one line per paragraph.
function textValue(content: DraftContent, flowedText: boolean): string {
  const text = content.text.replaceAll(NUL, "");
  return flowedText ? text : writeFixed(unflow(text, true));
}

// The properties of a draft's create (RFC 8621 section 4.6); an empty
// address list or reply header goes not at all.
export type EmailCreate = {
  mailboxIds: Record<string, true>;
  keywords: Record<string, true>;
  from?: MailAddress[];
  to?: MailAddress[];
  cc?: MailAddress[];
  bcc?: MailAddress[];
  subject?: string;
  inReplyTo?: string[];
  references?: string[];
  bodyStructure: CreatePart;
  bodyValues: Record<string, { value: string }>;
};

// The Email/set create of a draft (RFC 8621 section 4.6): the tree as
// `bodyStructure` with the text parts in `bodyValues` and every other
// part by its blob, filed in Drafts alone with $draft and $seen, from the
// identity. The server sets the Message-ID, the dates and the transfer
// encodings.
export function createObject(content: DraftContent, context: CreateContext): EmailCreate {
  const values = {
    [TEXT_PART]: { value: textValue(content, context.flowedText) },
    ...(content.mode === "rich"
      ? { [HTML_PART]: { value: content.html.replaceAll(NUL, "") } }
      : {}),
  };
  return {
    mailboxIds: { [context.draftsId]: true },
    keywords: { $draft: true, $seen: true },
    ...headersOf(content),
    bodyStructure: treeOf(content, context.flowedText),
    bodyValues: values,
  };
}

// The header fields of a create, each where it holds something.
function headersOf(
  content: DraftContent,
): Omit<EmailCreate, "mailboxIds" | "keywords" | "bodyStructure" | "bodyValues"> {
  const from = headerAddress(content.identity);
  const subject = headerText(content.subject).trim();
  const fields = {
    from: from === null ? [] : [from],
    to: addressesOf(content.to),
    cc: addressesOf(content.cc),
    bcc: addressesOf(content.bcc),
    inReplyTo: messageIds(content.inReplyTo),
    references: messageIds(content.references),
  };
  const held = Object.entries(fields).filter(([, values]) => values.length > 0);
  return { ...Object.fromEntries(held), ...(subject === "" ? {} : { subject }) };
}
