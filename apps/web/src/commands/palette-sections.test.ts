// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { COMMANDS } from "./fixtures";
import { sectionsOf } from "./palette-sections";
import type { Command } from "./registry";

function labels(sections: ReturnType<typeof sectionsOf>): Record<string, string[]> {
  return Object.fromEntries(
    sections.map((section) => [section.value, section.items.map((entry) => entry.label)]),
  );
}

test("an empty query lists every command by group, the recent ones first", () => {
  const sections = sectionsOf(COMMANDS, ["go.drafts", "list.open", "gone"], "", "en");
  expect(sections.map((section) => section.label)).toEqual([
    "Recent",
    "Navigate",
    "Go",
    "Act",
    "App",
  ]);
  expect(labels(sections)["recent"]).toEqual(["Go to Drafts", "Open conversation"]);
  expect(labels(sections)["go"]).toEqual([
    "Go to Inbox",
    "Go to Drafts",
    "Go to Facturen",
    "Go to 2024",
  ]);
  expect(sectionsOf(COMMANDS, [], "", "en")[0]?.label).toBe("Navigate");
});

test("a query keeps the matches best first and drops the groups left empty", () => {
  const sections = sectionsOf(COMMANDS, ["go.drafts"], "co", "en");
  // Every conversation command matches on a word; the palette matches on its start.
  expect(labels(sections)).toEqual({
    navigate: [
      "Next conversation",
      "Previous conversation",
      "Open conversation",
      "Close conversation",
    ],
    app: ["Command palette", "Switch account"],
  });
  expect(labels(sectionsOf(COMMANDS, [], "s", "en"))["app"]).toEqual([
    "Switch account",
    "Keyboard shortcuts",
  ]);
  expect(labels(sectionsOf(COMMANDS, [], "  inb ", "en"))).toEqual({ go: ["Go to Inbox"] });
});

test("a query finds a command by its keys and reads past accents and case", () => {
  expect(labels(sectionsOf(COMMANDS, [], "gi", "en"))).toEqual({ go: ["Go to Inbox"] });
  expect(labels(sectionsOf(COMMANDS, [], "ctrl+k", "en"))).toEqual({ app: ["Command palette"] });
  expect(labels(sectionsOf(COMMANDS, [], "FACTUREN", "en"))).toEqual({ go: ["Go to Facturen"] });
  expect(labels(sectionsOf(COMMANDS, [], "é", "en"))["navigate"]).toContain("Next conversation");
  expect(sectionsOf(COMMANDS, [], "zzz", "en")).toEqual([]);
});

test("a label and a query find each other whatever Unicode form each one holds", () => {
  const facturen = COMMANDS.find((command) => command.id === "go.facturen");
  if (facturen === undefined) {
    throw new Error("the fixtures miss a command");
  }
  const composed = "Go to Reçu".normalize("NFC");
  const decomposed = composed.normalize("NFD");
  const holdsDecomposed: Command[] = [
    ...COMMANDS,
    { ...facturen, id: "go.recu", label: decomposed, keys: [] },
  ];
  const holdsComposed: Command[] = [
    ...COMMANDS,
    { ...facturen, id: "go.recu", label: composed, keys: [] },
  ];
  expect(labels(sectionsOf(holdsDecomposed, [], "recu", "en"))).toEqual({ go: [decomposed] });
  expect(labels(sectionsOf(holdsDecomposed, [], "reçu", "en"))).toEqual({ go: [decomposed] });
  expect(labels(sectionsOf(holdsDecomposed, [], "go to reçu", "en"))).toEqual({
    go: [decomposed],
  });
  expect(labels(sectionsOf(holdsComposed, [], "reçu".normalize("NFD"), "en"))).toEqual({
    go: [composed],
  });
});

test("a query of punctuation alone finds the commands whose keys or label hold it and nothing else", () => {
  expect(labels(sectionsOf(COMMANDS, [], "?", "en"))).toEqual({ app: ["Keyboard shortcuts"] });
  expect(labels(sectionsOf(COMMANDS, [], ".", "en"))).toEqual({ act: ["Show new mail"] });
  expect(sectionsOf(COMMANDS, [], "+", "en").flatMap((section) => section.items)).toHaveLength(2);
});

test("a command registered again keeps its place in its group, with a query as without", () => {
  const next = COMMANDS.find((command) => command.id === "list.next");
  const palette = COMMANDS.find((command) => command.id === "palette.open");
  if (next === undefined || palette === undefined) {
    throw new Error("the fixtures miss a command");
  }
  const again: Command[] = [...COMMANDS, { ...next }, { ...palette }];
  const navigate = [
    "Next conversation",
    "Previous conversation",
    "Open conversation",
    "Close conversation",
  ];
  expect(labels(sectionsOf(again, [], "", "en"))["navigate"]).toEqual(navigate);
  expect(labels(sectionsOf(again, [], "", "en"))["app"]).toEqual([
    "Command palette",
    "Keyboard shortcuts",
    "Switch account",
  ]);
  expect(labels(sectionsOf(again, [], "co", "en"))["navigate"]).toEqual(navigate);
});

test("two registrations of one id list once, the later one", () => {
  const first = COMMANDS[0];
  if (first === undefined) {
    throw new Error("the fixtures hold no command");
  }
  const twice: Command[] = [...COMMANDS, { ...first, label: "Newer" }];
  const listed = labels(sectionsOf(twice, [], "", "en"))["navigate"];
  expect(listed).not.toContain("Next conversation");
  expect(listed).toContain("Newer");
});
