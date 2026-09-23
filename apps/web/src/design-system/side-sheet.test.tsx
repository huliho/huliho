// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { SideSheet } from "./side-sheet";

afterEach(() => {
  cleanup();
});

test("an open sheet names itself and its Close button asks to close it", () => {
  const onOpenChange = vi.fn<(open: boolean) => void>();
  render(
    <SideSheet open onOpenChange={onOpenChange} label="Mailboxes" closeLabel="Close">
      <p>inside</p>
    </SideSheet>,
  );
  expect(screen.getByRole("dialog", { name: "Mailboxes" }).textContent).toContain("inside");
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onOpenChange.mock.calls.at(0)?.[0]).toBe(false);
});

test("a closed sheet renders nothing", () => {
  render(
    <SideSheet
      open={false}
      onOpenChange={vi.fn<(open: boolean) => void>()}
      label="Mailboxes"
      closeLabel="Close"
    >
      <p>inside</p>
    </SideSheet>,
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
