// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { dispatchKey } from "../commands/registry";
import { ToastProvider, Toasts, toastManager } from "./toast";

function shown(): void {
  render(
    <ToastProvider>
      <Toasts />
    </ToastProvider>,
  );
}

// What one toast holds beside its sentence: the icon, Undo, the key and the seconds.
async function toastOf(text: string): Promise<HTMLElement> {
  const found = (await screen.findByText(text)).parentElement;
  if (found === null) {
    throw new Error(`no toast says ${text}`);
  }
  return found;
}

afterEach(() => {
  act(() => {
    toastManager.close();
  });
  cleanup();
});

test("of two toasts that can undo, the newest carries the key and answers it; the older keeps its seconds", async () => {
  shown();
  const older = vi.fn<() => void>();
  const newer = vi.fn<() => void>();
  act(() => {
    toastManager.add({ description: "Archived.", data: { undo: older } });
    toastManager.add({ description: "Deleted.", data: { undo: newer } });
  });
  expect(within(await toastOf("Deleted.")).queryByText("z")).not.toBeNull();
  const archived = await toastOf("Archived.");
  expect(within(archived).queryByText("z")).toBeNull();
  expect(within(archived).getByText("5s")).toBeDefined();
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key: "z" }));
  });
  expect(newer).toHaveBeenCalledOnce();
  expect(older).not.toHaveBeenCalled();
});

test("a refusal carries the alert circle and no Undo", async () => {
  shown();
  act(() => {
    toastManager.add({
      description: "Couldn’t archive. Your mail is safe.",
      data: { tone: "danger" },
    });
  });
  const refusal = await toastOf("Couldn’t archive. Your mail is safe.");
  expect(refusal.querySelector("svg")).not.toBeNull();
  expect(within(refusal).queryByRole("button", { name: "Undo" })).toBeNull();
});
