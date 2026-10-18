// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test, vi } from "vitest";

import { lendOpenThreadRun, runOnOpenThread } from "./open-thread";

test("a press runs on the list that lends last, and an earlier one taking back leaves it standing", () => {
  const first = vi.fn<(id: string) => void>();
  const second = vi.fn<(id: string) => void>();
  const takeFirst = lendOpenThreadRun(first);
  const takeSecond = lendOpenThreadRun(second);
  takeFirst();
  runOnOpenThread("thread.archive");
  expect(second).toHaveBeenCalledExactlyOnceWith("thread.archive");
  expect(first).not.toHaveBeenCalled();
  takeSecond();
  runOnOpenThread("thread.delete");
  expect(second).toHaveBeenCalledOnce();
});
