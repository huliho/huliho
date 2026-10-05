// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "./coordinator";
import { useMailCache } from "./use-mail-cache";

const detach = vi.hoisted(() => vi.fn<() => void>());
const attach = vi.hoisted(() => vi.fn<(lease: Lease) => () => void>(() => detach));
vi.mock("./client", () => ({ attachCache: attach }));

const ACCOUNTS = { accounts: [{ id: "acc-1" }, { id: "acc-2" }], probeIntervalMinutes: 15 };

function Harness() {
  useMailCache(null);
  return null;
}

// The query client tells its observers in a later task.
function settled(): Promise<void> {
  return act(
    () =>
      new Promise<void>((resolve) => {
        setTimeout(resolve);
      }),
  );
}

// The hook with the accounts the guard fetched; the session answer is
// the test's to give.
async function mounted(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.accounts, ACCOUNTS);
  render(
    <QueryClientProvider client={queryClient}>
      <Harness />
    </QueryClientProvider>,
  );
  await settled();
  return queryClient;
}

async function answers(queryClient: QueryClient, privacyStrict: boolean): Promise<void> {
  queryClient.setQueryData(queryKeys.session, { privacyStrict });
  await settled();
}

beforeEach(() => {
  // No request of the test answers, so a query holds what the test gave it.
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(() => new Promise<Response>(() => undefined)),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  attach.mockClear();
  detach.mockClear();
});

test("no lease goes out before the session answer says how the instance keeps mail", async () => {
  const queryClient = await mounted();
  expect(attach).not.toHaveBeenCalled();
  await answers(queryClient, true);
  expect(attach).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ accounts: ["acc-1", "acc-2"], watching: null, strict: true }),
  );
});

test.each([true, false])(
  "the lease names the privacy setting of the session answer: %j",
  async (strict) => {
    const queryClient = await mounted();
    await answers(queryClient, strict);
    expect(attach.mock.lastCall?.[0].strict).toBe(strict);
  },
);

test("a session answer that names another setting ends the lease and sends a new one", async () => {
  const queryClient = await mounted();
  await answers(queryClient, false);
  expect(detach).not.toHaveBeenCalled();
  await answers(queryClient, true);
  expect(detach).toHaveBeenCalledOnce();
  expect(attach).toHaveBeenCalledTimes(2);
  expect(attach.mock.lastCall?.[0].strict).toBe(true);
});
