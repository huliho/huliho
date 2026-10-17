// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A tab holds a lock of this name and its own id while a destroy it
// asked waits held, so every worker of the origin can tell it lives.
export const LEASE_LOCK = "huliho-lease:";

// A lock held for the length of one call that answers what the call
// answered; the locks the origin holds now.
export interface Locks {
  request<Value>(name: string, run: () => Promise<Value>): Promise<Value>;
  // The names held that start with `prefix`, without it.
  held(prefix: string): Promise<Set<string>>;
}

// The Web Locks API loses the callback's type on the way back; this keeps it.
export function webLocks(manager: Pick<LockManager, "request" | "query">): Locks {
  return {
    request: async <Value>(name: string, run: () => Promise<Value>): Promise<Value> => {
      const settled = Promise.withResolvers<Value>();
      await manager.request(name, () => run().then(settled.resolve, settled.reject));
      return settled.promise;
    },
    held: async (prefix) => {
      const { held = [] } = await manager.query();
      return new Set(
        held.flatMap(({ name }) =>
          name?.startsWith(prefix) === true ? [name.slice(prefix.length)] : [],
        ),
      );
    },
  };
}
