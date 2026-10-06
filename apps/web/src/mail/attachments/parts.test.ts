// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail } from "@huliho/core";
import { expect, test } from "vitest";

import { DOWNLOAD, DOWNLOAD_PREFIX, message, part } from "../body/frame-rig";
import {
  NAME_TAIL_CHARS,
  attachmentsOf,
  familyOf,
  fileNameOf,
  fileSize,
  isDangerous,
  splitName,
} from "./parts";

const NONE: ReadonlySet<string> = new Set();
const LOGO = part("image/png", {
  partId: "3",
  blobId: "b-logo",
  name: "logo.png",
  cid: "logo@shop.example",
});
// A format character and a zero-width space, which a saved name loses.
const OVERRIDE = String.fromCodePoint(0x202e);
const ZERO_WIDTH = String.fromCodePoint(0x200b);

function detail(parts: Parameters<typeof message>[1], html = "<p>x</p>"): BodyDetail {
  return { body: message([html], parts), download: DOWNLOAD };
}

test("a part's icon follows its type family; an SVG and an unknown type are files", () => {
  expect(familyOf("image/png")).toBe("image");
  expect(familyOf("IMAGE/JPEG")).toBe("image");
  expect(familyOf("image/svg+xml")).toBe("file");
  expect(familyOf("application/pdf")).toBe("document");
  expect(familyOf("text/plain")).toBe("document");
  expect(familyOf("application/vnd.openxmlformats-officedocument.wordprocessingml.document")).toBe(
    "document",
  );
  expect(familyOf("application/zip")).toBe("archive");
  expect(familyOf("audio/mpeg")).toBe("media");
  expect(familyOf("video/mp4")).toBe("media");
  expect(familyOf("message/rfc822")).toBe("message");
  expect(familyOf("application/octet-stream")).toBe("file");
});

test("a part downloads under its name; an attached message as a file of its own kind", () => {
  expect(fileNameOf(part("application/pdf", { name: " offerte.pdf " }))).toBe("offerte.pdf");
  expect(fileNameOf(part("application/pdf"))).toBe("attachment");
  expect(fileNameOf(part("message/rfc822"))).toBe("message.eml");
  expect(fileNameOf(part("message/rfc822", { name: "Re: hello" }))).toBe("Re: hello.eml");
  expect(fileNameOf(part("message/rfc822", { name: "Fwd.EML" }))).toBe("Fwd.EML");
});

test("a saved name loses the characters the route drops, so an extension cannot hide behind one", () => {
  const hidden = part("application/pdf", { name: `run.exe${ZERO_WIDTH}` });
  expect(fileNameOf(hidden)).toBe("run.exe");
  expect(isDangerous(fileNameOf(hidden))).toBe(true);
  const turned = part("application/pdf", { name: `photo${OVERRIDE}gnp.exe` });
  expect(fileNameOf(turned)).toBe("photognp.exe");
  expect(fileNameOf(part("application/pdf", { name: "a\u0000b.pdf" }))).toBe("ab.pdf");
});

test("a name whose extension can run a program is told by its last extension, as a system reads it", () => {
  const asking = ["run.exe", "RUN.EXE", "report.pdf.exe", "run.exe. ", "viewer.html", "x.svg"];
  expect(asking.filter((name) => isDangerous(name))).toEqual(asking);
  const plain = ["notes.txt", "archive.tar.gz", "noext", ".htaccess", "photo.png"];
  expect(plain.filter((name) => isDangerous(name))).toEqual([]);
});

test("a size reads as a file manager shows it, in the reader's language", () => {
  expect(fileSize(1, "en")).toBe("1 byte");
  expect(fileSize(512, "en")).toBe("512 bytes");
  expect(fileSize(1024, "en")).toBe("1 kB");
  expect(fileSize(48_210, "en")).toBe("48 kB");
  expect(fileSize(2_400_000, "en")).toBe("2.4 MB");
  expect(fileSize(1_500_000_000, "en")).toBe("1.5 GB");
  expect(fileSize(2_400_000, "nl")).toBe("2,4 MB");
});

test("a size that rounds up to a thousand reads in the next unit", () => {
  expect(fileSize(999_400, "en")).toBe("999 kB");
  expect(fileSize(999_600, "en")).toBe("1 MB");
  expect(fileSize(999_940_000, "en")).toBe("999.9 MB");
  expect(fileSize(999_950_000, "en")).toBe("1 GB");
});

test("a name is cut in the middle with its last characters whole", () => {
  expect(splitName("Offerte_badkamer_renovatie_v3_definitief.pdf")).toEqual([
    "Offerte_badkamer_renovatie_v3_",
    "definitief.pdf",
  ]);
  expect(splitName("short.pdf")).toEqual(["", "short.pdf"]);
  const emoji = `${"🙂".repeat(NAME_TAIL_CHARS)}.png`;
  const [start, end] = splitName(emoji);
  expect(start + end).toBe(emoji);
  expect(end.isWellFormed()).toBe(true);
});

test("the strip holds every attachment but one the document shows inline, and the media parts of the body list", () => {
  const audio = part("audio/mpeg", { blobId: "b-voice", name: "voice.m4a", size: 900_000 });
  const doc = part("application/pdf", { blobId: "b-doc", name: "a.pdf", size: 10 });
  const body = detail([LOGO, doc]);
  body.body.htmlBody.push(audio);
  const shown = attachmentsOf(body, new Set(["3"]), "en");
  expect(shown.map((one) => one.label)).toEqual(["a.pdf", "voice.m4a"]);
  const kept = attachmentsOf(body, NONE, "en");
  expect(kept.map((one) => one.label)).toEqual(["logo.png", "a.pdf", "voice.m4a"]);
  expect(kept.map((one) => one.preview)).toEqual([
    `${DOWNLOAD_PREFIX}b-logo/logo.png?type=image%2Fpng`,
    null,
    null,
  ]);
  // A part without a blob has nothing to download.
  const bare = detail([part("application/pdf", { name: "x.pdf" })]);
  expect(attachmentsOf(bare, NONE, "en")).toEqual([]);
});

test("a part names itself or is called by what it is, and downloads as bytes under a safe name", () => {
  const parts = [
    part("application/pdf", {
      blobId: "b-1",
      name: "Offerte_badkamer_renovatie_v3_definitief.pdf",
    }),
    part("application/octet-stream", { blobId: "b-2", size: 1024 }),
    part("message/rfc822", { blobId: "b-3", size: 12_400 }),
    part("text/html", { blobId: "b-4", name: "factuur_viewer.html" }),
  ];
  const [pdf, unnamed, forwarded, viewer] = attachmentsOf(detail(parts), NONE, "en");
  expect(pdf).toMatchObject({
    download: `${DOWNLOAD_PREFIX}b-1/Offerte_badkamer_renovatie_v3_definitief.pdf?type=application%2Foctet-stream`,
    preview: null,
    dangerous: false,
    family: "document",
  });
  expect(unnamed).toMatchObject({
    label: "Unnamed attachment",
    size: "1 kB",
    download: `${DOWNLOAD_PREFIX}b-2/attachment?type=application%2Foctet-stream`,
  });
  expect(forwarded).toMatchObject({
    label: "Attached message",
    family: "message",
    download: `${DOWNLOAD_PREFIX}b-3/message.eml?type=application%2Foctet-stream`,
  });
  expect(viewer).toMatchObject({ dangerous: true, family: "document" });
  const dutch = attachmentsOf(detail(parts), NONE, "nl");
  expect(dutch.map((one) => one.label).slice(1, 3)).toEqual([
    "Bijlage zonder naam",
    "Bijgevoegd bericht",
  ]);
});

test("a raster part under a name that asks first never previews, so no picture downloads past the question", () => {
  const parts = [
    part("image/png", { blobId: "b-5", name: "kaart.html", size: 20_000 }),
    part("image/png", { blobId: "b-6", name: "kaart.png", size: 20_000 }),
  ];
  const [asks, previews] = attachmentsOf(detail(parts), NONE, "en");
  expect(asks).toMatchObject({ dangerous: true, preview: null, family: "image" });
  expect(previews).toMatchObject({
    dangerous: false,
    preview: `${DOWNLOAD_PREFIX}b-6/kaart.png?type=image%2Fpng`,
  });
});
