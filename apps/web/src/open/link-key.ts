// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

const LINK_KEY = "huliho-link-key";

// The key this device writes into every link it makes from a mail. The
// open route follows a link with the key at once and asks first for any
// other, so an address someone else made up never forwards a visitor
// through this app unseen.
export function linkKey(): string {
  const held = localStorage.getItem(LINK_KEY);
  if (held !== null && held !== "") {
    return held;
  }
  const made = crypto.randomUUID();
  localStorage.setItem(LINK_KEY, made);
  return made;
}

// Whether a link carries this device's key. A device that holds none
// made no link.
export function hasLinkKey(key: string): boolean {
  const held = localStorage.getItem(LINK_KEY);
  return held !== null && held !== "" && held === key;
}

// A session that ends in this browser takes the key with it, so the
// links of a mail still on screen ask before they open.
export function forgetLinkKey(): void {
  localStorage.removeItem(LINK_KEY);
}
