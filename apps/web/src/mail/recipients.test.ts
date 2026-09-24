// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { THREAD } from "./fixtures";
import { displayName, recipientNames, recipientsIn, senderOf } from "./recipients";

const NEWEST = THREAD.emails["e-3"];

if (NEWEST === undefined) {
  throw new Error("the fixture thread has no newest message");
}

// The words as the locale lists them, which the test never spells out itself.
function phrase(locale: string, words: string[]): string {
  return new Intl.ListFormat(locale, { type: "conjunction" }).format(words);
}

test("a sender shows by name with the address beside it, by address alone without a name", () => {
  expect(senderOf(NEWEST)).toEqual({
    name: "Pieter Blom",
    address: "pieter@blom-installaties.example",
  });
  expect(senderOf({ ...NEWEST, from: [{ name: "", email: "a@example.test" }] })).toEqual({
    name: "a@example.test",
    address: null,
  });
  expect(
    senderOf({ ...NEWEST, from: null, sender: [{ name: null, email: "b@example.test" }] }),
  ).toEqual({ name: "b@example.test", address: null });
  expect(senderOf({ ...NEWEST, from: null })).toEqual({ name: null, address: null });
});

test("the recipients read as one phrase in the locale, nothing when there are none", () => {
  const names = [
    { name: "Sanne Bakker", email: "sanne@fastmail.com" },
    { name: null, email: "jonas@kastanje.example" },
    { name: "Ruben Smit", email: "ruben@kastanje.example" },
  ];
  expect(recipientNames(names, "en")).toBe(
    phrase("en", ["Sanne Bakker", "jonas@kastanje.example", "Ruben Smit"]),
  );
  expect(recipientNames(names.slice(0, 2), "nl")).toBe("Sanne Bakker en jonas@kastanje.example");
  expect(recipientNames(null, "en")).toBe("");
  expect(recipientNames([], "en")).toBe("");
});

test("a field the message does not carry lists nobody", () => {
  expect(recipientsIn(NEWEST, "cc").map(displayName)).toEqual(["Jonas Verhulst"]);
  expect(recipientsIn(NEWEST, "bcc")).toEqual([]);
  expect(recipientsIn(NEWEST, "replyTo")).toEqual([]);
});
