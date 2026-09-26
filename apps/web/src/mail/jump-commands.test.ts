// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { expect, test, vi } from "vitest";

import { MAILBOXES } from "./fixtures";
import { jumpCommands } from "./jump-commands";

test("every mailbox gets a jump in the tree's order, with the letter the tree shows", () => {
  const jump = vi.fn<(mailboxId: string) => void>();
  const commands = jumpCommands(MAILBOXES, "en", jump);
  expect(
    commands.map((command) => [command.label, command.keys.map((chord) => chord.key)]),
  ).toEqual([
    ["Go to Inbox", ["g", "i"]],
    ["Go to Drafts", ["g", "d"]],
    ["Go to Sent", ["g", "s"]],
    ["Go to Archive", ["g", "a"]],
    ["Go to Junk", ["g", "j"]],
    ["Go to Trash", ["g", "t"]],
    ["Go to Facturen", ["g", "f"]],
    ["Go to Verbouwing", ["g", "v"]],
    ["Go to Offertes", ["g", "o"]],
  ]);
  expect(commands.every((command) => command.group === "go")).toBe(true);
  commands[1]?.run();
  expect(jump).toHaveBeenCalledWith("mb-drafts");
});

test("a folder whose name has no free letter is listed without keys", () => {
  const model = MAILBOXES[0];
  if (model === undefined) {
    throw new Error("the fixtures hold no mailbox");
  }
  const numbered: Mailbox = { ...model, id: "mb-2024", name: "2024", role: null, sortOrder: 12 };
  const commands = jumpCommands(
    [...MAILBOXES, numbered],
    "en",
    vi.fn<(mailboxId: string) => void>(),
  );
  expect(commands.at(-1)).toMatchObject({ id: "go.mb-2024", label: "Go to 2024", keys: [] });
});
