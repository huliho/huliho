// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { MockInstance } from "vitest";

import type { RefusedMessage } from "../../cache/client";
import { ToastProvider, Toasts, toastManager } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { showActionToast } from "./action-toast";
import { showRefused } from "./refusals";

const UNDO = { inverses: [], held: [], threads: ["t1"] };
const REFUSED: RefusedMessage = {
  kind: "refused",
  accountId: "a1",
  emailIds: ["e1"],
  verbs: ["archive"],
};

function shown(): void {
  render(
    <ToastProvider>
      <Toasts />
    </ToastProvider>,
  );
}

// Two action toasts, of e1 and of e9; answers a spy on the manager's
// close from then on.
async function twoActions(): Promise<MockInstance<typeof toastManager.close>> {
  const cache = fixtureCache({});
  const added = vi.spyOn(toastManager, "add");
  act(() => {
    showActionToast({ cache, accountId: "a1", text: "Archived.", undo: UNDO, emailIds: ["e1"] });
    showActionToast({ cache, accountId: "a1", text: "Deleted.", undo: UNDO, emailIds: ["e9"] });
  });
  await screen.findByText("Deleted.");
  expect(added).toHaveBeenCalledTimes(2);
  return vi.spyOn(toastManager, "close");
}

// One action toast over the two emails of a thread; answers a spy on
// the manager's close from then on.
async function oneThread(): Promise<MockInstance<typeof toastManager.close>> {
  const cache = fixtureCache({});
  act(() => {
    showActionToast({
      cache,
      accountId: "a1",
      text: "Archived.",
      undo: UNDO,
      emailIds: ["e1", "e2"],
    });
  });
  await screen.findByText("Archived.");
  return vi.spyOn(toastManager, "close");
}

afterEach(() => {
  vi.restoreAllMocks();
  act(() => {
    toastManager.close();
  });
  cleanup();
});

test("a refusal closes the toast of the action it names alone and says the mail is safe", async () => {
  shown();
  const closed = await twoActions();
  act(() => {
    showRefused(REFUSED, "en", true);
  });
  expect(closed).toHaveBeenCalledOnce();
  expect(await screen.findByText("Couldn’t archive. Your mail is safe.")).toBeDefined();
});

test("a refusal of one email of a thread leaves its toast with Undo and says the mail is safe", async () => {
  shown();
  const closed = await oneThread();
  act(() => {
    showRefused(REFUSED, "en", true);
  });
  expect(closed).not.toHaveBeenCalled();
  expect(await screen.findByText("Couldn’t archive. Your mail is safe.")).toBeDefined();
  expect(screen.getByRole("button", { name: "Undo" })).toBeDefined();
});

test("a refusal of the last email of a thread closes its toast", async () => {
  shown();
  const closed = await oneThread();
  act(() => {
    showRefused(REFUSED, "en", false);
  });
  expect(closed).not.toHaveBeenCalled();
  act(() => {
    showRefused({ ...REFUSED, emailIds: ["e2"] }, "en", false);
  });
  expect(closed).toHaveBeenCalledOnce();
});

test("a tab that does not tell still closes the action's toast", async () => {
  shown();
  const closed = await twoActions();
  act(() => {
    showRefused(REFUSED, "en", false);
  });
  expect(closed).toHaveBeenCalledOnce();
  expect(screen.queryByText("Couldn’t archive. Your mail is safe.")).toBeNull();
});
