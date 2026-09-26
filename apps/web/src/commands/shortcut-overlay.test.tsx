// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { COMMANDS } from "./fixtures";
import { ShortcutOverlay } from "./shortcut-overlay";

afterEach(cleanup);

test("the overlay lists every keyed command by group and names the key that closes it", () => {
  render(
    <ShortcutOverlay
      open
      onOpenChange={vi.fn<(open: boolean) => void>()}
      locale="en"
      commands={COMMANDS}
    />,
  );
  const dialog = screen.getByRole("dialog", { name: "Keyboard" });
  expect(
    within(dialog).getByText("Esc closes this. Keys never fire while you type."),
  ).toBeDefined();
  expect(
    within(dialog)
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent),
  ).toEqual(["Navigate", "Go", "Act", "App"]);
  expect(within(dialog).getByText("Go to Inbox").nextElementSibling?.textContent).toBe("g i");
  expect(within(dialog).getByText("Switch account").nextElementSibling?.textContent).toBe(
    "Ctrl+Shift+L",
  );
  expect(within(dialog).queryByText("Go to 2024")).toBeNull();
  expect(within(dialog).getByText(/a folder takes the first free letter/)).toBeDefined();
});

test("a command registered again keeps its row in its column", () => {
  const open = COMMANDS.find((command) => command.id === "list.open");
  if (open === undefined) {
    throw new Error("the fixtures miss a command");
  }
  render(
    <ShortcutOverlay
      open
      onOpenChange={vi.fn<(open: boolean) => void>()}
      locale="en"
      commands={[{ ...open }, ...COMMANDS, { ...open }]}
    />,
  );
  const column = screen.getByRole("heading", { level: 3, name: "Navigate" }).parentElement;
  if (column === null) {
    throw new Error("the column is not rendered");
  }
  expect(
    within(column)
      .getAllByRole("term")
      .map((term) => term.textContent),
  ).toEqual([
    "Next conversation",
    "Previous conversation",
    "Open conversation",
    "Close conversation",
  ]);
});

test("a group without a keyed command is not drawn", () => {
  render(
    <ShortcutOverlay
      open
      onOpenChange={vi.fn<(open: boolean) => void>()}
      locale="en"
      commands={COMMANDS.filter((command) => command.group !== "act")}
    />,
  );
  expect(screen.queryByRole("heading", { level: 3, name: "Act" })).toBeNull();
});
