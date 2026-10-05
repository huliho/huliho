// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { htmlDetail } from "./body-fixtures";
import { cutOf, cutSize, frameTitle, messageDownload } from "./body-view";
import { THREAD } from "./fixtures";

const NEWEST = THREAD.emails["e-3"];

if (NEWEST === undefined) {
  throw new Error("the fixture thread has no newest message");
}

const WHOLE = { show: () => undefined, pending: false };

test("the size a cut body was asked at reads in megabytes, in the locale", () => {
  const small = htmlDetail("e-3", "<p>x</p>", { cut: true }).body;
  expect(cutSize(small, "en")).toBe("4 MB");
  expect(cutSize({ ...small, large: true }, "en")).toBe("12 MB");
  expect(cutSize(small, "nl")).toBe("4 MB");
});

test("a whole body offers nothing; a cut one the second ask; one cut at the large cap the download", () => {
  expect(cutOf(htmlDetail("e-3", "<p>x</p>"), NEWEST, "en", WHOLE)).toBeNull();
  const once = cutOf(htmlDetail("e-3", "<p>x</p>", { cut: true }), NEWEST, "en", WHOLE);
  expect(once).toMatchObject({
    kind: "size",
    size: "4 MB",
    whole: WHOLE.show,
    pending: false,
    download: null,
  });
  const twice = cutOf(
    htmlDetail("e-3", "<p>x</p>", { cut: true, large: true }),
    NEWEST,
    "en",
    WHOLE,
  );
  expect(twice).toEqual({
    kind: "size",
    size: "12 MB",
    whole: null,
    pending: false,
    download: "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822",
  });
});

test("the whole message downloads under the email's own blob id", () => {
  const detail = htmlDetail("e-3", "<p>x</p>");
  expect(messageDownload(detail, { ...NEWEST, blobId: "blob/with slash" })).toBe(
    "/api/jmap/acc-1/download/u1/blob%2Fwith%20slash/message.eml?type=message%2Frfc822",
  );
});

test("the frame is titled by who wrote and what about, with a fallback for an empty subject", () => {
  expect(frameTitle(NEWEST, "en")).toBe(
    "Message from Pieter Blom about Re: Offerte badkamerrenovatie, herziene versie",
  );
  expect(frameTitle({ ...NEWEST, subject: "  " }, "en")).toBe("Message from Pieter Blom");
  expect(frameTitle({ ...NEWEST, subject: null, from: null }, "nl")).toBe(
    "Bericht van Onbekende afzender",
  );
});
