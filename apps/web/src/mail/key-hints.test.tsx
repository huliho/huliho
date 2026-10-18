// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { COMMANDS } from "../commands/fixtures";
import { registerCommand } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { KeyHints } from "./key-hints";

const layout = { wide: true };
const cleanups: (() => void)[] = [];

beforeEach(() => {
  layout.wide = true;
  stubWidthQueries(layout);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  for (const off of cleanups.splice(0)) {
    off();
  }
});

function registerAll(ids: string[]): void {
  for (const command of COMMANDS.filter((entry) => ids.includes(entry.id))) {
    cleanups.push(registerCommand(command));
  }
}

test("the strip names the keys the registry holds and follows a registration", () => {
  registerAll(["list.next", "list.previous", "shortcuts.open"]);
  render(<KeyHints locale="en" hidden={false} />);
  expect(screen.getByText("j/k").parentElement?.textContent).toBe("j/k move");
  expect(screen.getByText("?").parentElement?.textContent).toBe("? shortcuts");
  expect(screen.queryByText("open")).toBeNull();
  act(() => {
    registerAll(["list.open"]);
  });
  expect(screen.getByText("o").parentElement?.textContent).toBe("o open");
});

test("archive joins the strip after open while the mailbox offers it", () => {
  registerAll(["list.next", "list.previous", "list.open", "shortcuts.open"]);
  const { container } = render(<KeyHints locale="en" hidden={false} />);
  expect(screen.queryByText("archive")).toBeNull();
  act(() => {
    cleanups.push(
      registerCommand({
        id: "thread.archive",
        label: "Archive",
        group: "act",
        keys: [{ key: "e" }],
        run: () => undefined,
      }),
    );
  });
  const hints = [...container.querySelectorAll("p > span")].map((hint) => hint.textContent);
  expect(hints).toEqual(["j/k move", "o open", "e archive", "? shortcuts"]);
});

test("a hint with one of its commands missing stays away; so does the strip on a phone or under the sync foot", () => {
  registerAll(["list.next", "shortcuts.open"]);
  const { rerender } = render(<KeyHints locale="en" hidden={false} />);
  expect(screen.queryByText("move")).toBeNull();
  expect(screen.getByText("shortcuts")).toBeDefined();
  rerender(<KeyHints locale="en" hidden />);
  expect(screen.queryByText("shortcuts")).toBeNull();
  layout.wide = false;
  cleanup();
  render(<KeyHints locale="en" hidden={false} />);
  expect(screen.queryByText("shortcuts")).toBeNull();
});
