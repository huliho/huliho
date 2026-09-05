// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { UserRow } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { UserTable } from "./user-table";
import type { UserTableProps } from "./user-table";

type Reset = (user: UserRow) => void;

const NOW = new Date("2026-05-14T10:00:00");
const DAY_MS = 86_400_000;
const JONAS: UserRow = {
  id: "u-2",
  name: "Jonas",
  login: "jonas@example.com",
  role: "member",
  lastActiveAt: NOW.getTime() - DAY_MS,
};
const ROWS: UserRow[] = [
  {
    id: "u-1",
    name: "Mira",
    login: "mira@example.com",
    role: "owner",
    lastActiveAt: NOW.getTime(),
  },
  JONAS,
  { id: "u-3", name: "Noor", login: "noor@example.com", role: "admin", lastActiveAt: null },
];

// jsdom has no matchMedia; the stub answers the sidebar query for one width.
function stubWidth(wide: boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: wide,
    media: query,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const OWNER = { id: "u-1", role: "owner" } as const;
const ADMIN = { id: "u-3", role: "admin" } as const;

function renderTable(wide: boolean, actor: UserTableProps["actor"] = OWNER): Mock<Reset> {
  stubWidth(wide);
  const onReset = vi.fn<Reset>();
  render(<UserTable rows={ROWS} locale="en" now={NOW} actor={actor} onReset={onReset} />);
  return onReset;
}

test("a wide screen gets the table with its headers and the own row says You", () => {
  const onReset = renderTable(true);
  expect(screen.getByRole("table")).toBeDefined();
  expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual([
    "Name",
    "Sign-in name",
    "Role",
    "Last active",
    "",
  ]);
  expect(screen.getByText("You")).toBeDefined();
  expect(screen.getByText("active now")).toBeDefined();
  expect(screen.getByText("yesterday")).toBeDefined();
  expect(screen.getByText("Never")).toBeDefined();
  expect(screen.queryByRole("button", { name: "Reset password for Mira" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Reset password for Jonas" }));
  expect(onReset).toHaveBeenCalledExactlyOnceWith(JONAS);
});

test("an admin gets no reset on a row above their role", () => {
  renderTable(true, ADMIN);
  expect(screen.getByText("You")).toBeDefined();
  expect(screen.queryByRole("button", { name: "Reset password for Mira" })).toBeNull();
  expect(screen.getByRole("button", { name: "Reset password for Jonas" })).toBeDefined();
});

test("a phone gets one card per user with the same facts", () => {
  renderTable(false);
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.getByRole("list", { name: "Users" })).toBeDefined();
  expect(screen.getAllByRole("listitem")).toHaveLength(ROWS.length);
  expect(screen.getByText("Owner")).toBeDefined();
  expect(screen.getByText("You")).toBeDefined();
  expect(screen.getAllByRole("button", { name: /^Reset password for/ })).toHaveLength(2);
});
