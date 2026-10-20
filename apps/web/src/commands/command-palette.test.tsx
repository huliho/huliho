// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { CommandPalette } from "./command-palette";
import { COMMANDS } from "./fixtures";
import type { MoveMode } from "./move-sections";
import type { Command } from "./registry";

// The picker's mailboxes in the tree's order, a nested one by its path.
const TARGETS: MoveMode["targets"] = [
  { id: "archive", name: "Archive", path: [] },
  { id: "facturen", name: "Facturen", path: [] },
  { id: "verbouwing", name: "Verbouwing", path: [] },
  { id: "offertes", name: "Offertes", path: ["Verbouwing"] },
  { id: "privé", name: "Privé", path: [] },
];

function renderMove(recent: string | null) {
  const onPick = vi.fn<(id: string) => void>();
  render(
    <CommandPalette
      open
      onOpenChange={() => undefined}
      locale="en"
      commands={COMMANDS}
      recent={[]}
      onRun={() => undefined}
      move={{ targets: TARGETS, recent, onPick: () => undefined }}
      onPick={onPick}
    />,
  );
  return { onPick, input: screen.getByRole("combobox", { name: "Move to…" }) };
}

interface Options {
  recent?: string[];
  defaultQuery?: string;
}

function renderPalette(options: Options = {}) {
  const onRun = vi.fn<(command: Command) => void>();
  const onOpenChange = vi.fn<(open: boolean) => void>();
  render(
    <CommandPalette
      open
      onOpenChange={onOpenChange}
      locale="en"
      commands={COMMANDS}
      recent={options.recent ?? []}
      defaultQuery={options.defaultQuery}
      onRun={onRun}
    />,
  );
  return { onRun, onOpenChange };
}

function listed(): string[] {
  return screen.getAllByRole("option").map((option) => option.textContent);
}

afterEach(cleanup);

test("the palette opens on its input with every command by group and the keys beside them", async () => {
  renderPalette({ recent: ["go.drafts"] });
  const dialog = screen.getByRole("dialog", { name: "Command palette" });
  const input = within(dialog).getByRole("combobox", { name: "Command palette" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(input);
  });
  expect(within(dialog).getByText("Recent")).toBeDefined();
  expect(listed()[0]).toBe("Go to Draftsg d");
  expect(listed()).toContain("Command paletteCtrl+K");
  expect(listed()).toContain("Go to 2024");
  expect(screen.getByText("every action lives here")).toBeDefined();
});

test("typing narrows the list, the first match is highlighted and Enter runs it", () => {
  const { onRun } = renderPalette();
  const input = screen.getByRole("combobox", { name: "Command palette" });
  fireEvent.change(input, { target: { value: "dra" } });
  expect(listed()).toEqual(["Go to Draftsg d"]);
  const highlighted = screen.getByRole("option", { name: /Go to Drafts/ });
  expect(highlighted.hasAttribute("data-highlighted")).toBe(true);
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onRun).toHaveBeenCalledOnce();
  expect(onRun.mock.calls[0]?.[0].id).toBe("go.drafts");
});

test("a query nothing answers says so; a click on a row runs it", () => {
  const { onRun } = renderPalette({ defaultQuery: "zzz" });
  expect(screen.queryAllByRole("option")).toEqual([]);
  expect(screen.getByText("No command matches.")).toBeDefined();
  const input = screen.getByRole("combobox", { name: "Command palette" });
  fireEvent.change(input, { target: { value: "switch" } });
  fireEvent.click(screen.getByRole("option", { name: /Switch account/ }));
  expect(onRun.mock.calls[0]?.[0].id).toBe("account.switch");
});

test("the move mode names itself, lists the last target under Recent and keeps it in the tree below", () => {
  const { onPick, input } = renderMove("facturen");
  expect(screen.getByRole("dialog", { name: "Move to…" })).toBeDefined();
  expect(input.getAttribute("placeholder")).toBe("Type a mailbox name");
  expect(screen.getByText("Recent")).toBeDefined();
  expect(screen.getByText("Mailboxes")).toBeDefined();
  expect(listed()).toEqual([
    "Facturen",
    "Archive",
    "Facturen",
    "Verbouwing",
    "Verbouwing / Offertes",
    "Privé",
  ]);
  expect(screen.getAllByRole("option")[0]?.hasAttribute("data-highlighted")).toBe(true);
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onPick).toHaveBeenCalledExactlyOnceWith("facturen");
});

test("a remembered mailbox outside the targets stands under no Recent", () => {
  renderMove("gone");
  expect(screen.queryByText("Recent")).toBeNull();
  expect(listed()).toEqual(["Archive", "Facturen", "Verbouwing", "Verbouwing / Offertes", "Privé"]);
  expect(screen.getAllByRole("option")[0]?.hasAttribute("data-highlighted")).toBe(true);
});

test("typing keeps the names that hold the query in the tree's order, with accents compared by their base", () => {
  const { onPick, input } = renderMove(null);
  fireEvent.change(input, { target: { value: "ve" } });
  expect(listed()).toEqual(["Archive", "Verbouwing", "Privé"]);
  expect(screen.queryByText("Recent")).toBeNull();
  fireEvent.click(screen.getByRole("option", { name: "Privé" }));
  expect(onPick).toHaveBeenCalledExactlyOnceWith("privé");
  fireEvent.change(input, { target: { value: "zzz" } });
  expect(screen.queryAllByRole("option")).toEqual([]);
  expect(screen.getByRole("status").textContent).toBe("No mailbox matches.");
});
