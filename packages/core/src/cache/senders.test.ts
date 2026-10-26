// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { EmailHeader } from "../jmap/schemas";
import { ACCOUNT, at, email } from "./fake-jmap";
import { SENDER_SUGGESTIONS } from "./limits";
import { MemoryMailStore } from "./memory";
import { foldText, readSenders, sendersOf, startsAWord } from "./senders";

function from(id: string, seconds: number, name: string | null, address: string): EmailHeader {
  return { ...email(id, { receivedAt: at(seconds) }), from: [{ name, email: address }] };
}

async function held(headers: readonly EmailHeader[]): Promise<MemoryMailStore> {
  const store = new MemoryMailStore();
  await store.commit(ACCOUNT, { emails: { put: headers } });
  return store;
}

test("one sender per address in any case, with the newest spelling and the name the newest named header gave", () => {
  expect(
    sendersOf([
      from("e1", 1, "Sanne Bakker", "sanne@example.test"),
      from("e2", 3, null, "Sanne@Example.test"),
      from("e3", 2, "Sanne B.", "sanne@example.test"),
      { ...email("e4", { receivedAt: at(4) }), from: null },
      from("e5", 5, "", "sanne@example.test"),
    ]),
  ).toEqual([{ email: "sanne@example.test", name: "Sanne B.", count: 4, newest: at(5) }]);
});

test("a sender matches where its name, its address or a word of either starts with the prefix, in any case and without marks", async () => {
  const store = await held([
    from("e1", 1, "Zoë de Vries", "zdv@kastanje.example"),
    from("e2", 2, "Pieter Blom", "pieter@blom-installaties.example"),
    from("e3", 3, null, "noreply@service.example"),
  ]);
  const emails = async (prefix: string): Promise<string[]> =>
    (await readSenders(store, ACCOUNT, prefix)).map((sender) => sender.email);
  expect(await emails("ZOE DE")).toEqual(["zdv@kastanje.example"]);
  expect(await emails("zoë")).toEqual(["zdv@kastanje.example"]);
  expect(await emails("vri")).toEqual(["zdv@kastanje.example"]);
  expect(await emails("de vr")).toEqual(["zdv@kastanje.example"]);
  expect(await emails("install")).toEqual(["pieter@blom-installaties.example"]);
  expect(await emails("noreply@serv")).toEqual(["noreply@service.example"]);
  expect(await emails("x")).toEqual([]);
});

test("most mail first, then the newest, at most the suggestions' cap; an empty prefix matches every sender", async () => {
  const headers = Array.from({ length: SENDER_SUGGESTIONS + 2 }, (_, index) =>
    from(`e${String(index)}`, index, `Person ${String(index)}`, `p${String(index)}@example.test`),
  );
  const store = await held([...headers, from("x1", 0, "Person 0", "p0@example.test")]);
  const found = await readSenders(store, ACCOUNT, "  ");
  expect(found).toHaveLength(SENDER_SUGGESTIONS);
  expect(found.map((sender) => sender.email)).toEqual([
    "p0@example.test",
    "p6@example.test",
    "p5@example.test",
    "p4@example.test",
    "p3@example.test",
  ]);
});

test("a text matches from its start or a word's start, in any case and without marks", () => {
  expect(foldText("Zoë ÉCOLE")).toBe("zoe ecole");
  expect(startsAWord("Projet École", "ecol")).toBe(true);
  expect(startsAWord("Blom-Installatie", "INST")).toBe(true);
  expect(startsAWord("Offertes", "ffer")).toBe(false);
  expect(startsAWord("Offertes", "")).toBe(true);
});
