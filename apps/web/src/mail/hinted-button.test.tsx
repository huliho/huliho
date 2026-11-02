// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { RenderOptions } from "@testing-library/react";
import { StrictMode } from "react";
import type { ReactElement } from "react";
import { afterEach, expect, test, vi } from "vitest";

import { HintedButton } from "./hinted-button";

function Tooltip({ children }: { children: ReactElement }) {
  return <span data-testid="hinted">{children}</span>;
}

const chunk = vi.hoisted(() => ({ ready: Promise.withResolvers<unknown>() }));

vi.mock("./menus-chunk", () => ({ prefetchMenus: () => chunk.ready.promise }));

async function drawn(
  onClick: () => void = vi.fn<() => void>(),
  wrapper?: RenderOptions["wrapper"],
): Promise<HTMLElement> {
  await act(async () => {
    render(
      <HintedButton
        label="New folder"
        keys={[]}
        align="end"
        button={<button onClick={onClick}>New folder</button>}
      />,
      { wrapper },
    );
    await Promise.resolve();
  });
  return screen.getByRole("button", { name: "New folder" });
}

// The chunk lands and the hinted button stands; the swap takes one wait.
async function landed(): Promise<HTMLElement> {
  await act(async () => {
    chunk.ready.resolve({ Tooltip });
    await chunk.ready.promise;
  });
  const box = await screen.findByTestId("hinted");
  return within(box).getByRole("button", { name: "New folder" });
}

afterEach(() => {
  cleanup();
  chunk.ready = Promise.withResolvers<unknown>();
});

test("a focus on the bare button moves to the hinted one when the chunk lands", async () => {
  const bare = await drawn();
  bare.focus();
  const hinted = await landed();
  expect(hinted).not.toBe(bare);
  expect(document.activeElement).toBe(hinted);
});

test("a focus elsewhere stays there when the chunk lands", async () => {
  const other = document.createElement("button");
  document.body.append(other);
  await drawn();
  other.focus();
  await landed();
  expect(document.activeElement).toBe(other);
  other.remove();
});

test("a press on the bare button waits and runs once on the hinted one", async () => {
  const onClick = vi.fn<() => void>();
  const bare = await drawn(onClick);
  fireEvent.click(bare);
  expect(onClick).not.toHaveBeenCalled();
  await landed();
  expect(onClick).toHaveBeenCalledTimes(1);
});

// StrictMode runs the hinted button's layout effect twice on mount.
test("a press on the bare button runs once on the hinted one under StrictMode", async () => {
  const onClick = vi.fn<() => void>();
  const bare = await drawn(onClick, StrictMode);
  fireEvent.click(bare);
  await landed();
  expect(onClick).toHaveBeenCalledTimes(1);
});

test("a press on the hinted button runs at once", async () => {
  const onClick = vi.fn<() => void>();
  await drawn(onClick);
  const hinted = await landed();
  expect(onClick).not.toHaveBeenCalled();
  fireEvent.click(hinted);
  expect(onClick).toHaveBeenCalledTimes(1);
});
