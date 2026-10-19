// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { registerCommand } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { lendSelection } from "./actions/selection-lend";
import { FASTMAIL, INBOX_ID, MAILBOXES } from "./fixtures";
import { prefetchMenus } from "./menus-chunk";
import { PaneHeader } from "./pane-header";

const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
const NAME = INBOX?.name ?? "";

function nothing(): void {
  // The test's x toggles nothing.
}

// The list's key that picks a row, as a list that lets threads be picked registers it.
function pickable(): () => void {
  return registerCommand({
    id: "selection.toggle",
    label: "Select",
    group: "act",
    keys: [{ key: "x" }],
    run: nothing,
  });
}

async function header(): Promise<void> {
  await act(async () => {
    await prefetchMenus();
  });
  render(<PaneHeader locale="en" account={FASTMAIL} mailbox={INBOX} pending={false} />);
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("on touch the mailbox's name opens its menu, whose Select opens the selection", async () => {
  stubWidthQueries({ wide: true, coarse: true });
  const start = vi.fn<() => void>();
  const lent = lendSelection({ start, close: () => false });
  const unregister = pickable();
  try {
    await header();
    const trigger = await screen.findByRole("button", { name: NAME });
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    expect(screen.getByRole("heading", { level: 1 }).contains(trigger)).toBe(true);
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Select" }));
    expect(start).toHaveBeenCalledOnce();
  } finally {
    unregister();
    lent();
  }
});

test("the name stays plain under a mouse alone and where no thread can be picked", async () => {
  stubWidthQueries({ wide: true });
  const unregister = pickable();
  await header();
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(NAME);
  expect(screen.queryByRole("button")).toBeNull();
  unregister();
  cleanup();
  stubWidthQueries({ wide: true, coarse: true });
  await header();
  expect(screen.queryByRole("button")).toBeNull();
});
