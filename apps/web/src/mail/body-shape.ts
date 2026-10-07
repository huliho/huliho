// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailBody, EmailBodyPart } from "@huliho/core";

const HTML_TYPE = "text/html";
const TEXT_TYPE = "text/";
const IMAGE_TYPE = "image/";

// The one shape a message takes when the server could not describe it:
// a single part of this type and name over the whole message, no text
// and no HTML list. The name is also the one the whole message
// downloads under.
const WHOLE_MESSAGE_TYPE = "application/octet-stream";
export const WHOLE_MESSAGE_NAME = "message.eml";

// How the card shows a message: its body list in the frame, text in
// the card, nothing to show or a message too complex to describe.
export type BodyShape = "html" | "text" | "none" | "complex";

function typeOf(part: EmailBodyPart): string {
  return part.type.toLowerCase();
}

function isText(part: EmailBodyPart): boolean {
  return typeOf(part).startsWith(TEXT_TYPE);
}

// A text part that is not the HTML, which the text list carries as well
// for a message with HTML alone (RFC 8621 section 4.1.4).
function isPlainText(part: EmailBodyPart): boolean {
  return isText(part) && typeOf(part) !== HTML_TYPE;
}

// Whether the frame has to draw the part: HTML, or an image the sender
// placed among the text, which a text block cannot show.
function needsFrame(part: EmailBodyPart): boolean {
  const type = typeOf(part);
  return type === HTML_TYPE || type.startsWith(IMAGE_TYPE);
}

function isWholeMessage(part: EmailBodyPart): boolean {
  return typeOf(part) === WHOLE_MESSAGE_TYPE && part.name === WHOLE_MESSAGE_NAME;
}

// Whether the message carries a plain text part, whatever else it carries.
export function hasPlainText(body: EmailBody): boolean {
  return body.textBody.some(isPlainText);
}

export function shapeOf(body: EmailBody): BodyShape {
  if (body.htmlBody.some(needsFrame)) {
    return "html";
  }
  if (hasPlainText(body)) {
    return "text";
  }
  const whole = isWholeMessage(body.bodyStructure);
  return whole && body.htmlBody.length === 0 ? "complex" : "none";
}

// The text a message shows as plain text: the values of its plain text
// parts in order, each on lines of its own.
export function textOf(body: EmailBody): string {
  return body.textBody
    .filter(isPlainText)
    .flatMap((part) => (part.partId === null ? [] : [body.bodyValues[part.partId]?.value]))
    .filter((value) => value !== undefined)
    .join("\n");
}

// Whether a value of the body was cut at the cap it was asked at.
export function isCut(body: EmailBody): boolean {
  return Object.values(body.bodyValues).some((value) => value.isTruncated);
}
