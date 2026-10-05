// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

const LINK_KEY = "huliho-link-key";

// The components that read the key, told when this tab makes or forgets it.
const readers = new Set<() => void>();

function changed(): void {
  for (const reader of readers) {
    reader();
  }
}

// The key this device holds, null for a device that holds none.
function held(): string | null {
  const key = localStorage.getItem(LINK_KEY);
  return key === null || key === "" ? null : key;
}

// The key this device writes into every link it makes from a mail, made
// on a device that holds none. The open route follows a link with the
// key at once and asks first for any other, so an address someone else
// made up never forwards a visitor through this app unseen.
export function linkKey(): string {
  const key = held();
  if (key !== null) {
    return key;
  }
  const made = crypto.randomUUID();
  localStorage.setItem(LINK_KEY, made);
  changed();
  return made;
}

// Whether a link carries this device's key. A device that holds none
// made no link.
export function hasLinkKey(key: string): boolean {
  return held() === key;
}

// A session that ends in this browser takes the key with it, so the
// links of a mail still on screen ask before they open.
export function forgetLinkKey(): void {
  localStorage.removeItem(LINK_KEY);
  changed();
}

function subscribe(onChange: () => void): () => void {
  readers.add(onChange);
  window.addEventListener("storage", onChange);
  return () => {
    readers.delete(onChange);
    window.removeEventListener("storage", onChange);
  };
}

// The key as a component that writes links reads it: null once a
// session ended here, in this tab or another.
export function useLinkKey(): string | null {
  return useSyncExternalStore(subscribe, held, () => null);
}
