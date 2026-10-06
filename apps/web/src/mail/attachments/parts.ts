// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { downloadUrl } from "@huliho/core";
import type { BodyDetail, EmailBody, EmailBodyPart } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// The six raster types a browser renders, which the download route
// answers inline when the bytes agree; every other type is a download.
const RASTER_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
  "image/avif",
  "image/bmp",
]);

// What a chip asks of the route: bytes to save, whatever the part says it is.
const OCTET_STREAM = "application/octet-stream";
const MESSAGE_TYPE = "message/rfc822";
const MESSAGE_EXTENSION = ".eml";
// The names a part without one downloads under.
const UNNAMED_FILE = "attachment";
const UNNAMED_MESSAGE_FILE = "message.eml";

// A name with one of these extensions can run a program on the reader's
// computer, so its download asks first.
const DANGEROUS_EXTENSIONS = new Set([
  "exe",
  "msi",
  "bat",
  "cmd",
  "com",
  "scr",
  "pif",
  "js",
  "jse",
  "vbs",
  "vbe",
  "wsf",
  "ps1",
  "hta",
  "jar",
  "apk",
  "dmg",
  "pkg",
  "iso",
  "lnk",
  "reg",
  "sh",
  "html",
  "htm",
  "svg",
  "xml",
]);

// The characters the route drops from a saved name: controls and every
// character the Unicode property Default_Ignorable_Code_Point names.
// They leave the shown name as well, so the name reads as it saves and
// an extension cannot hide behind one of them.
const UNSEEN = /[\p{Cc}\p{Default_Ignorable_Code_Point}]/gu;

// How many characters at the end of a name stay when it is cut in the
// middle, so the extension stays readable.
export const NAME_TAIL_CHARS = 14;

// The icon a part gets. An image is a raster type alone; an SVG and an
// unknown type are files.
export type Family = "image" | "document" | "archive" | "media" | "message" | "file";

const DOCUMENT_TYPES = new Set(["application/pdf", "application/rtf", "application/msword"]);
const DOCUMENT_PREFIXES = [
  "text/",
  "application/vnd.openxmlformats-officedocument.",
  "application/vnd.oasis.opendocument.",
  "application/vnd.ms-",
];
const ARCHIVE_TYPES = new Set([
  "application/zip",
  "application/x-zip-compressed",
  "application/x-7z-compressed",
  "application/x-tar",
  "application/gzip",
  "application/x-gzip",
  "application/x-rar-compressed",
  "application/vnd.rar",
  "application/x-bzip2",
]);
const MEDIA_PREFIXES = ["audio/", "video/"];

// One part of the strip with what the card shows and where it downloads.
export interface Attachment {
  // The part's place in the strip, which keys it.
  key: string;
  // What the chip reads.
  label: string;
  size: string;
  family: Family;
  // The route with the bytes as a download.
  download: string;
  // The route with the declared type, for a raster part under a safe
  // name; null otherwise.
  preview: string | null;
  dangerous: boolean;
}

function isRaster(type: string): boolean {
  return RASTER_TYPES.has(type.toLowerCase());
}

function isMedia(type: string): boolean {
  return MEDIA_PREFIXES.some((prefix) => type.toLowerCase().startsWith(prefix));
}

export function familyOf(type: string): Family {
  const lower = type.toLowerCase();
  if (RASTER_TYPES.has(lower)) {
    return "image";
  }
  if (lower.startsWith("message/")) {
    return "message";
  }
  if (isMedia(lower)) {
    return "media";
  }
  if (ARCHIVE_TYPES.has(lower)) {
    return "archive";
  }
  const document =
    DOCUMENT_TYPES.has(lower) || DOCUMENT_PREFIXES.some((prefix) => lower.startsWith(prefix));
  return document ? "document" : "file";
}

function isMessage(part: EmailBodyPart): boolean {
  return part.type.toLowerCase() === MESSAGE_TYPE;
}

// The part's name as it saves: without the characters the route drops.
function nameOf(part: EmailBodyPart): string {
  return (part.name ?? "").replace(UNSEEN, "").trim();
}

// The name a part saves under: its own, an attached message with its
// extension, else a plain word.
export function fileNameOf(part: EmailBodyPart): string {
  const name = nameOf(part);
  if (name === "") {
    return isMessage(part) ? UNNAMED_MESSAGE_FILE : UNNAMED_FILE;
  }
  const bare = isMessage(part) && !name.toLowerCase().endsWith(MESSAGE_EXTENSION);
  return bare ? `${name}${MESSAGE_EXTENSION}` : name;
}

// Whether a saved name's extension is on the list. Trailing dots and
// spaces leave first, as an operating system drops them from a name.
export function isDangerous(fileName: string): boolean {
  const name = fileName.replace(/[\s.]+$/u, "");
  const dot = name.lastIndexOf(".");
  return dot >= 0 && DANGEROUS_EXTENSIONS.has(name.slice(dot + 1).toLowerCase());
}

const KILO = 1000;

// A unit with the decimals it shows and the bytes one of it holds. Bytes
// and kilobytes show whole, the larger units with one decimal.
interface Unit {
  name: string;
  digits: number;
  bytes: number;
}

const UNITS: Unit[] = [
  { name: "byte", digits: 0, bytes: 1 },
  { name: "kilobyte", digits: 0, bytes: KILO },
  { name: "megabyte", digits: 1, bytes: KILO * KILO },
];
const LARGEST: Unit = { name: "gigabyte", digits: 1, bytes: KILO * KILO * KILO };

// The value as it reads once rounded to a unit's decimals.
function rounded(value: number, digits: number): number {
  return Number(value.toFixed(digits));
}

// A size as a file manager shows it, in the reader's locale. The unit is
// the first whose rounded value stays under a thousand, so no size reads
// as a thousand of a unit. Bytes take the long unit word, since its short
// form has no plural.
export function fileSize(bytes: number, locale: Locale): string {
  const unit = UNITS.find((one) => rounded(bytes / one.bytes, one.digits) < KILO) ?? LARGEST;
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: unit.name,
    unitDisplay: unit.name === "byte" ? "long" : "short",
    maximumFractionDigits: unit.digits,
  }).format(bytes / unit.bytes);
}

// A name in two pieces for a cut in the middle: the end keeps its last
// characters whole while the start gives way.
export function splitName(name: string): [string, string] {
  const chars = Array.from(name);
  const at = Math.max(0, chars.length - NAME_TAIL_CHARS);
  return [chars.slice(0, at).join(""), chars.slice(at).join("")];
}

// The parts the strip shows, in the message's order: every attachment
// but one the document shows inline, then the audio and video parts of
// the body list, which the frame draws nothing for.
function stripParts(body: EmailBody, inline: ReadonlySet<string>): EmailBodyPart[] {
  const attached = body.attachments.filter(
    (part) => part.partId === null || !inline.has(part.partId),
  );
  const held = new Set(attached.map((part) => part.blobId));
  const media = body.htmlBody.filter((part) => isMedia(part.type) && !held.has(part.blobId));
  return [...attached, ...media];
}

function labelOf(part: EmailBodyPart, locale: Locale): string {
  const name = nameOf(part);
  if (name !== "") {
    return name;
  }
  return isMessage(part)
    ? m.attachment_message({}, { locale })
    : m.attachment_unnamed({}, { locale });
}

// Every part of the strip with what the card needs to show and to
// download it. A part without a blob has nothing to download and is
// left out. A raster part previews under a safe name alone: a name that
// asks first stays a chip, so no picture ever downloads past the question.
export function attachmentsOf(
  detail: BodyDetail,
  inline: ReadonlySet<string>,
  locale: Locale,
): Attachment[] {
  return stripParts(detail.body, inline).flatMap((part, index): Attachment[] => {
    if (part.blobId === null) {
      return [];
    }
    const fileName = fileNameOf(part);
    const dangerous = isDangerous(fileName);
    const blob = { blobId: part.blobId, name: fileName };
    const previews = isRaster(part.type) && !dangerous;
    return [
      {
        key: String(index),
        label: labelOf(part, locale),
        size: fileSize(part.size, locale),
        family: familyOf(part.type),
        download: downloadUrl(detail.download, { ...blob, type: OCTET_STREAM }),
        preview: previews ? downloadUrl(detail.download, { ...blob, type: part.type }) : null,
        dangerous,
      },
    ];
  });
}
