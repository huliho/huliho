// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test, vi } from "vitest";

import { chunk } from "./chunk";

// The fields `use` reads off a promise it does not have to wait for.
interface Tracked extends Promise<unknown> {
  status?: string;
  value?: unknown;
  reason?: unknown;
}

test("the import starts once, every call answers the same promise and the outcome lands on it", async () => {
  const answer = Promise.withResolvers<string>();
  const load = vi.fn<() => Promise<string>>(() => answer.promise);
  const prefetch = chunk(load);
  const first: Tracked = prefetch();
  expect(prefetch()).toBe(first);
  expect(load).toHaveBeenCalledOnce();
  expect(first.status).toBe("pending");
  answer.resolve("the module");
  await expect(prefetch()).resolves.toBe("the module");
  await vi.waitFor(() => {
    expect(first.status).toBe("fulfilled");
  });
  expect(first.value).toBe("the module");
  expect(prefetch()).toBe(first);
});

test("an import that fails carries its reason, is handed over once and the next call starts it again", async () => {
  const failure = new Error("offline");
  const load = vi
    .fn<() => Promise<string>>()
    .mockRejectedValueOnce(failure)
    .mockResolvedValue("the module");
  const prefetch = chunk(load);
  const refused: Tracked = prefetch();
  await expect(refused).rejects.toBe(failure);
  await vi.waitFor(() => {
    expect(refused.status).toBe("rejected");
  });
  expect(refused.reason).toBe(failure);
  expect(prefetch()).toBe(refused);
  const again = prefetch();
  expect(again).not.toBe(refused);
  expect(load).toHaveBeenCalledTimes(2);
  await expect(again).resolves.toBe("the module");
});
