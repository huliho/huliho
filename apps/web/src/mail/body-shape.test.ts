// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { message, part } from "./body/frame-rig";
import { isCut, shapeOf, textOf } from "./body-shape";

const TEXT = part("text/plain", { partId: "1", blobId: "b1" });
const OTHER_TEXT = part("text/plain", { partId: "3", blobId: "b3" });
const HTML = part("text/html", { partId: "2", blobId: "b2" });
const PDF = part("application/pdf", { blobId: "b4", name: "offer.pdf" });

function valued(ids: [string, string][]) {
  return Object.fromEntries(
    ids.map(([partId, value]) => [partId, { value, isEncodingProblem: false, isTruncated: false }]),
  );
}

test("a message with an HTML part renders as HTML, whatever its text parts", () => {
  const body = { ...message([HTML]), textBody: [TEXT], bodyValues: valued([["2", "<p>hi</p>"]]) };
  expect(shapeOf(body)).toBe("html");
  // A text part standing in for the HTML part (RFC 8621 section 4.1.4) is text.
  expect(shapeOf({ ...message([TEXT]), textBody: [TEXT] })).toBe("text");
});

test("an image the sender placed among the text takes the body list to the frame", () => {
  const photo = part("image/jpeg", { partId: "4", blobId: "b5", name: "photo.jpg" });
  const withPhoto = { ...message([TEXT, photo]), textBody: [TEXT, photo] };
  expect(shapeOf(withPhoto)).toBe("html");
  expect(shapeOf({ ...message([photo]), textBody: [photo] })).toBe("html");
});

test("a message with text parts alone renders them in order, each on its own lines", () => {
  const body = {
    ...message([TEXT]),
    htmlBody: [],
    textBody: [TEXT, part("image/png", { partId: "4", blobId: "b5" }), OTHER_TEXT],
    bodyValues: valued([
      ["1", "first"],
      ["3", "third"],
    ]),
  };
  expect(shapeOf(body)).toBe("text");
  expect(textOf(body)).toBe("first\nthird");
  // A text part whose value never came is left out.
  expect(textOf({ ...body, bodyValues: valued([["1", "first"]]) })).toBe("first");
});

test("a message without text or HTML has nothing to show; the one the server could not describe is too complex", () => {
  const attachmentAlone = { ...message([]), attachments: [PDF] };
  expect(shapeOf(attachmentAlone)).toBe("none");
  const whole = part("application/octet-stream", { blobId: "e1", name: "message.eml", size: 4321 });
  const complex = { ...message([]), bodyStructure: whole, attachments: [whole] };
  expect(shapeOf(complex)).toBe("complex");
  // A single-part message that is one binary attachment has nothing to show,
  // whatever type its sender gave it.
  const binary = { ...message([]), bodyStructure: PDF, attachments: [PDF] };
  expect(shapeOf(binary)).toBe("none");
  const untyped = part("application/octet-stream", { blobId: "b6", name: "backup.bin" });
  expect(shapeOf({ ...message([]), bodyStructure: untyped, attachments: [untyped] })).toBe("none");
});

test("a body is cut when any of its values was", () => {
  const whole = message(["<p>hi</p>"]);
  expect(isCut(whole)).toBe(false);
  const cut = {
    ...whole,
    bodyValues: { "1": { value: "<p>h", isEncodingProblem: false, isTruncated: true } },
  };
  expect(isCut(cut)).toBe(true);
});
