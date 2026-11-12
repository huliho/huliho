// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { asciiAddress } from "../address";
import { draftContent } from "../cache/fake-jmap";
import { isAddress } from "./addresses";
import { refusalOf } from "./guards";
import {
  MESSAGE_IDS_MAX,
  MESSAGE_ID_BYTES,
  answerSubject,
  messageIds,
  replyHeaders,
  replyRecipients,
} from "./headers";
import type { MailAddress } from "./headers";

const OWN = "sanne@example.test";

function address(email: string, name: string | null = null): MailAddress {
  return { name, email };
}

test.each([
  ["Lunch", "Re: Lunch"],
  ["Re: Lunch", "Re: Lunch"],
  ["RE: re: Fwd: Lunch", "Re: Lunch"],
  ["Re[2]: Lunch", "Re: Lunch"],
  ["fw : Lunch", "Re: Lunch"],
  ["[team] Re: Lunch", "Re: [team] Lunch"],
  ["Re: [team] Re: Lunch", "Re: [team] Lunch"],
  ["Reply needed", "Re: Reply needed"],
  ["Re:", "Re:"],
  ["", "Re:"],
])("a reply to %j is %j, rfc5256_2_1", (subject, expected) => {
  expect(answerSubject(subject, "reply")).toBe(expected);
});

test("a forward writes Fwd once over the stripped subject and a missing subject reads empty", () => {
  expect(answerSubject("Re: Fwd: Lunch", "forward")).toBe("Fwd: Lunch");
  expect(answerSubject(null, "forward")).toBe("Fwd:");
});

const prefixes = fc.constantFrom("Re: ", "RE:", "re : ", "Fwd: ", "FW:", "Re[3]: ", "Fw [x]: ");
const tags = fc.constantFrom("", "[team] ", "[a b] ");
// The rest of a subject: no colon, so it opens with no prefix of its own.
const base = fc
  .string({ unit: fc.constantFrom("a", "b", " ", "é", "R", "e", "w", "f", "d"), minLength: 1 })
  .filter((text) => text.trim() !== "");

test("the prefix is written once whatever came before, and an answer of an answer stays as it is", () => {
  fc.assert(
    fc.property(fc.array(prefixes), tags, fc.array(prefixes), base, (before, tag, after, rest) => {
      const subject = `${before.join("")}${tag}${after.join("")}${rest}`;
      const reply = answerSubject(subject, "reply");
      expect(reply).toBe(`Re: ${tag}${rest.trimStart()}`.trimEnd());
      expect(answerSubject(reply, "reply")).toBe(reply);
      expect(answerSubject(answerSubject(subject, "forward"), "forward")).toBe(
        answerSubject(subject, "forward"),
      );
    }),
  );
});

test("a reply names the parent's id after its References, rfc5322_3_6_4", () => {
  const parent = {
    messageId: ["c@x.test"],
    inReplyTo: ["b@x.test"],
    references: ["a@x.test", "b@x.test"],
  };
  expect(replyHeaders(parent, "reply")).toEqual({
    inReplyTo: ["c@x.test"],
    references: ["a@x.test", "b@x.test", "c@x.test"],
  });
});

test("a parent without References lends its In-Reply-To where that names one id", () => {
  expect(
    replyHeaders(
      { messageId: ["c@x.test"], inReplyTo: ["b@x.test"], references: null },
      "replyAll",
    ),
  ).toEqual({
    inReplyTo: ["c@x.test"],
    references: ["b@x.test", "c@x.test"],
  });
  expect(
    replyHeaders(
      { messageId: ["c@x.test"], inReplyTo: ["a@x.test", "b@x.test"], references: null },
      "reply",
    ),
  ).toEqual({ inReplyTo: ["c@x.test"], references: ["c@x.test"] });
});

test("a parent with neither names its own id alone; a forward carries nothing", () => {
  const parent = { messageId: ["c@x.test"], inReplyTo: null, references: null };
  expect(replyHeaders(parent, "reply")).toEqual({
    inReplyTo: ["c@x.test"],
    references: ["c@x.test"],
  });
  expect(replyHeaders(parent, "forward")).toEqual({ inReplyTo: [], references: [] });
  expect(replyHeaders({ messageId: null, inReplyTo: null, references: null }, "reply")).toEqual({
    inReplyTo: [],
    references: [],
  });
});

test("an id a header cannot carry is left out and a long References keeps its first and newest ids", () => {
  expect(messageIds(["a@x.test", "with space@x", "<b@x.test>", "", "c\n@x", "d@x.test"])).toEqual([
    "a@x.test",
    "d@x.test",
  ]);
  const longest = `${"l".repeat(MESSAGE_ID_BYTES - "@x.test".length)}@x.test`;
  expect(messageIds([longest, `l${longest}`])).toEqual([longest]);
  const many = Array.from(
    { length: MESSAGE_IDS_MAX + 10 },
    (_, index) => `m${String(index)}@x.test`,
  );
  const kept = messageIds(many);
  expect(kept).toHaveLength(MESSAGE_IDS_MAX);
  expect(kept[0]).toBe("m0@x.test");
  expect(kept.at(-1)).toBe(many.at(-1));
  expect(kept[1]).toBe(`m${String(many.length - MESSAGE_IDS_MAX + 1)}@x.test`);
});

const parent = {
  from: [address("mo@x.test", "Mo")],
  replyTo: null,
  to: [address(OWN, "Sanne"), address("ali@x.test")],
  cc: [address("bo@x.test"), address("SANNE@example.test"), address("ali@x.test")],
};

test("a reply answers From, or Reply-To where the parent names one", () => {
  expect(replyRecipients(parent, "reply", OWN)).toEqual({
    to: [address("mo@x.test", "Mo")],
    cc: [],
  });
  const listed = { ...parent, replyTo: [address("list@x.test")] };
  expect(replyRecipients(listed, "reply", OWN).to).toEqual([address("list@x.test")]);
});

test("reply all adds To and keeps Cc, each address once and the account's own left out", () => {
  expect(replyRecipients(parent, "replyAll", OWN)).toEqual({
    to: [address("mo@x.test", "Mo"), address("ali@x.test")],
    cc: [address("bo@x.test")],
  });
});

test("an address a header cannot carry comes back as text, name and address together, rfc5322_3_4_1", () => {
  const odd = {
    from: [address("sänne@example.test", "Sänne")],
    replyTo: null,
    to: [address('"jo doe"@example.test'), address("mo@example.test")],
    cc: [address("bo@localhost", "Bo"), address("", "Undisclosed")],
  };
  expect(replyRecipients(odd, "reply", OWN)).toEqual({
    to: [{ text: "Sänne <sänne@example.test>" }],
    cc: [],
  });
  expect(replyRecipients(odd, "replyAll", OWN)).toEqual({
    to: [
      { text: "Sänne <sänne@example.test>" },
      { text: '"jo doe"@example.test' },
      address("mo@example.test"),
    ],
    cc: [{ text: "Bo <bo@localhost>" }, { text: "Undisclosed" }],
  });
});

// Pieces of an address, in and out of what a header carries.
const PIECES = ["@", ".", '"', "<", " ", "é", "a", "x.test", "jane@example.test"];
const addresses = fc.array(
  fc.record({
    name: fc.option(fc.string({ unit: fc.constantFrom("J", "é", " ", "\0"), maxLength: 4 })),
    email: fc.string({ unit: fc.constantFrom(...PIECES), maxLength: 6 }),
  }),
  { maxLength: 4 },
);

test("every recipient of a reply is an address a header carries or text, and Send takes no other", () => {
  fc.assert(
    fc.property(addresses, addresses, addresses, (from, to, cc) => {
      const given = { from, replyTo: null, to, cc };
      const { to: allTo, cc: allCc } = replyRecipients(given, "replyAll", OWN);
      const tokens = [...replyRecipients(given, "reply", OWN).to, ...allTo, ...allCc];
      for (const token of tokens.filter(isAddress)) {
        expect(asciiAddress(token.email)).not.toBeNull();
      }
      const refusal = refusalOf(draftContent({ to: allTo, cc: allCc }), null);
      const text = [...allTo, ...allCc].some((token) => !isAddress(token));
      expect(refusal?.kind === "notAnAddress").toBe(text);
    }),
  );
});

test("a reply to a message the account sent answers its To", () => {
  const sent = {
    from: [address("Sanne@Example.test")],
    replyTo: [address("elsewhere@x.test")],
    to: [address("mo@x.test")],
    cc: [address("bo@x.test")],
  };
  expect(replyRecipients(sent, "reply", OWN)).toEqual({ to: [address("mo@x.test")], cc: [] });
  expect(replyRecipients(sent, "replyAll", OWN)).toEqual({
    to: [address("mo@x.test")],
    cc: [address("bo@x.test")],
  });
});
