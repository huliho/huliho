// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { noteRecent, recentCommands } from "../commands/recent";
import { rememberLastAccount } from "../mail/last-account";
import { renderEnding } from "./session-end-rig";
import { useSignOut } from "./use-sign-out";

// The worker is not part of this test; the hook only has to ask it.
const clearCache = vi.hoisted(() => vi.fn<() => Promise<void>>(() => Promise.resolve()));
vi.mock("../cache/client", () => ({ clearCache }));

const LAST_ACCOUNT_KEY = "huliho-last-account";
const NO_CONTENT = 204;

function Harness() {
  const signOut = useSignOut("en");
  return (
    <button type="button" onClick={signOut}>
      sign out
    </button>
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  clearCache.mockClear();
  localStorage.clear();
});

test("signing out drops the cached mail, the remembered account and the commands last run, then goes to sign-in", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status: NO_CONTENT })),
  );
  rememberLastAccount("a1");
  noteRecent("go.mb-1");
  const { queryClient, router } = await renderEnding(Harness, true);
  fireEvent.click(screen.getByRole("button", { name: "sign out" }));
  await waitFor(() => {
    expect(router.state.location.pathname).toBe("/sign-in");
  });
  expect(clearCache).toHaveBeenCalledOnce();
  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(localStorage.getItem(LAST_ACCOUNT_KEY)).toBeNull();
  expect(recentCommands()).toEqual([]);
});
