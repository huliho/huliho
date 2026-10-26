// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Removes every key this device holds under the prefix, as a session
// that ends here takes its memories with it.
export function forgetStored(prefix: string): void {
  try {
    const keys = Array.from({ length: localStorage.length }, (_, index) => localStorage.key(index));
    for (const key of keys) {
      if (key?.startsWith(prefix) === true) {
        localStorage.removeItem(key);
      }
    }
  } catch {
    // A storage that refuses to be read holds nothing to forget.
  }
}
