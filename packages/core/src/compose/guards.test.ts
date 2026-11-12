// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { draftContent as content } from "../cache/fake-jmap";
import { LARGE_RECIPIENT_LIST, attachingWord, guardrailOf, refusalOf } from "./guards";

// The words of attaching each locale's catalog gives.
const EN = ["attach", "attached", "attachment", "attachments", "enclosed"];
const NL = ["bijlage", "bijlagen", "bijgevoegd", "bijgesloten"];

const NONE = new Set<never>();

const FILE = {
  blobId: "b1",
  name: "plan.pdf",
  type: "application/pdf",
  size: 10,
  cid: null,
  disposition: "attachment" as const,
};

test("a word of attaching stands as a whole word in either locale, in any case", () => {
  expect(attachingWord("See the Attached plan.", EN)).toBe("attached");
  expect(attachingWord("Zie de bijlage.", NL)).toBe("bijlage");
  expect(attachingWord("In de BIJLAGEN staat het.", NL)).toBe("bijlagen");
  expect(attachingWord("The attachment-free version", EN)).toBe("attachment");
});

test("a word inside another word or in the other locale finds nothing", () => {
  expect(attachingWord("He is unattached and reattaching.", EN)).toBeNull();
  expect(attachingWord("bijlagebestand", NL)).toBeNull();
  expect(attachingWord("Zie de bijlage.", EN)).toBeNull();
  expect(attachingWord("anything", [" ", ""])).toBeNull();
});

test("the attachment guardrail comes first and holds only while nothing is attached", () => {
  const draft = content({ subject: "" });
  expect(guardrailOf(draft, "see attached", EN, NONE)).toEqual({
    kind: "attachment",
    word: "attached",
  });
  expect(
    guardrailOf(content({ subject: "", attachments: [FILE] }), "see attached", EN, NONE),
  ).toEqual({
    kind: "subject",
  });
  const inline = { ...FILE, cid: "c1", disposition: "inline" as const };
  expect(guardrailOf(content({ attachments: [inline] }), "see attached", EN, NONE)?.kind).toBe(
    "attachment",
  );
});

test("a dismissed guardrail gives way to the next and the count counts all three fields", () => {
  const many = Array.from({ length: LARGE_RECIPIENT_LIST - 2 }, (_, index) => ({
    name: null,
    email: `p${String(index)}@x.test`,
  }));
  const draft = content({ subject: " ", cc: many, bcc: [{ text: "bob" }] });
  expect(guardrailOf(draft, "bijgevoegd", NL, new Set(["attachment"]))).toEqual({
    kind: "subject",
  });
  expect(guardrailOf(draft, "bijgevoegd", NL, new Set(["attachment", "subject"]))).toEqual({
    kind: "recipients",
    count: LARGE_RECIPIENT_LIST,
  });
  expect(guardrailOf(content({ cc: many.slice(1) }), "", NL, NONE)).toBeNull();
});

test("Send is refused for text that is no address, then for no recipient, then for size", () => {
  expect(refusalOf(content({ cc: [{ text: "bob" }] }), null)).toEqual({
    kind: "notAnAddress",
    text: "bob",
  });
  expect(refusalOf(content({ to: [] }), null)).toEqual({ kind: "noRecipient" });
  const heavy = content({ attachments: [FILE, { ...FILE, blobId: "b2" }] });
  expect(refusalOf(heavy, FILE.size * 2 - 1)).toEqual({ kind: "tooLarge" });
  expect(refusalOf(heavy, FILE.size * 2)).toBeNull();
  expect(refusalOf(heavy, null)).toBeNull();
});

test.each([
  [{ name: "Sänne", email: "sänne@example.test" }, "Sänne <sänne@example.test>"],
  [{ name: null, email: '"jo doe"@example.test' }, '"jo doe"@example.test'],
  [{ name: null, email: "mo@localhost" }, "mo@localhost"],
])(
  "Send is refused for the address token %j a header cannot carry, rfc5322_3_4_1",
  (token, text) => {
    const fit = { name: null, email: "mo@example.test" };
    expect(refusalOf(content({ to: [fit], bcc: [token] }), null)).toEqual({
      kind: "notAnAddress",
      text,
    });
  },
);
