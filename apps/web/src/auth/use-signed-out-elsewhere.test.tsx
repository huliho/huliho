// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { noteRecent, recentCommands } from "../commands/recent";
import { toastManager } from "../design-system/toast";
import { renderEnding } from "./session-end-rig";
import { useSignedOutElsewhere } from "./use-signed-out-elsewhere";

// The worker is not part of this test; the hook only has to ask it.
const clearCache = vi.hoisted(() => vi.fn<() => Promise<void>>(() => Promise.resolve()));
vi.mock("../cache/client", () => ({ clearCache }));

const LAST_ACCOUNT_KEY = "huliho-last-account";

function Harness() {
  const signedOutElsewhere = useSignedOutElsewhere();
  return (
    <button type="button" onClick={signedOutElsewhere}>
      elsewhere
    </button>
  );
}

afterEach(() => {
  cleanup();
  clearCache.mockClear();
  localStorage.clear();
});

test("a signed-in tab stops its worker, drops what it holds, its account and its recent commands and goes to sign-in", async () => {
  localStorage.setItem(LAST_ACCOUNT_KEY, "a1");
  noteRecent("go.mb-1");
  const { queryClient, router } = await renderEnding(Harness, true);
  fireEvent.click(screen.getByRole("button", { name: "elsewhere" }));
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/sign-in");
  });
  expect(clearCache).toHaveBeenCalledOnce();
  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(localStorage.getItem(LAST_ACCOUNT_KEY)).toBeNull();
  expect(recentCommands()).toEqual([]);
});

test("a second word while the tab is ending ends it once", async () => {
  const cleared = Promise.withResolvers<undefined>();
  clearCache.mockReturnValueOnce(cleared.promise);
  const added = vi.spyOn(toastManager, "add");
  const { router } = await renderEnding(Harness, true);
  fireEvent.click(screen.getByRole("button", { name: "elsewhere" }));
  fireEvent.click(screen.getByRole("button", { name: "elsewhere" }));
  cleared.resolve(undefined);
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/sign-in");
  });
  expect(clearCache).toHaveBeenCalledOnce();
  expect(added).toHaveBeenCalledOnce();
  added.mockRestore();
});

test("a tab without a session stays where it is", async () => {
  localStorage.setItem(LAST_ACCOUNT_KEY, "a1");
  noteRecent("go.mb-1");
  const { queryClient, router } = await renderEnding(Harness, false);
  fireEvent.click(screen.getByRole("button", { name: "elsewhere" }));
  expect(router.state.location.pathname).toBe("/");
  expect(clearCache).not.toHaveBeenCalled();
  expect(queryClient.getQueryData(queryKeys.mailboxes("a1"))).toEqual([]);
  expect(localStorage.getItem(LAST_ACCOUNT_KEY)).toBe("a1");
  expect(recentCommands()).toEqual(["go.mb-1"]);
});
