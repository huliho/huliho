// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { ToastProvider, Toasts, toastManager } from "../../design-system/toast";
import { flushPendingOnPageHide } from "../../undo/pending";
import { fixtureCache } from "../fixture-cache";
import { showActionToast } from "./action-toast";

const HELD = { inverses: [], held: [4, 5], threads: ["t1"] };
const MOVED = { inverses: [], held: [], threads: ["t1"] };

function shown(): void {
  render(
    <ToastProvider>
      <Toasts />
    </ToastProvider>,
  );
}

afterEach(() => {
  act(() => {
    toastManager.close();
  });
  cleanup();
});

test("a page that goes away lets the held destroys go once and closes their toast", async () => {
  shown();
  const cache = fixtureCache({});
  const off = flushPendingOnPageHide();
  act(() => {
    showActionToast({
      cache,
      accountId: "a1",
      text: "Deleted forever.",
      undo: HELD,
      emailIds: ["e1"],
    });
    showActionToast({ cache, accountId: "a1", text: "Archived.", undo: MOVED, emailIds: ["e2"] });
  });
  await screen.findByText("Archived.");
  act(() => {
    window.dispatchEvent(new Event("pagehide"));
    window.dispatchEvent(new Event("pagehide"));
  });
  expect(cache.released).toEqual([[4, 5]]);
  expect(cache.dropped).toEqual([]);
  await vi.waitFor(() => {
    expect(screen.queryByText("Deleted forever.")).toBeNull();
  });
  expect(screen.getByText("Archived.")).toBeDefined();
  act(() => {
    toastManager.close();
  });
  expect(cache.released).toEqual([[4, 5]]);
  off();
});
