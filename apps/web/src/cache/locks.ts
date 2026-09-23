// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A lock held for the length of one call, answered with what the call answered.
export interface Locks {
  request<Value>(name: string, run: () => Promise<Value>): Promise<Value>;
}

// The Web Locks API loses the callback's type on the way back; this keeps it.
export function webLocks(manager: Pick<LockManager, "request">): Locks {
  return {
    request: async <Value>(name: string, run: () => Promise<Value>): Promise<Value> => {
      const settled = Promise.withResolvers<Value>();
      await manager.request(name, () => run().then(settled.resolve, settled.reject));
      return settled.promise;
    },
  };
}
