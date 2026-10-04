// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailPatch, PendingRow } from "./store";

const KEYWORDS = "keywords/";

// The keyword a path names; a pointer token writes "/" as ~1 and "~" as
// ~0 (RFC 6901 section 3).
function keywordOf(path: string): string {
  if (!path.startsWith(KEYWORDS) || path.length === KEYWORDS.length) {
    throw new Error("a patch names keywords alone");
  }
  return path.slice(KEYWORDS.length).replaceAll("~1", "/").replaceAll("~0", "~");
}

// The keywords with a patch laid over them and the patch that takes it
// back: each path set to what the keywords held before.
export function patched(
  keywords: Record<string, true>,
  patch: EmailPatch,
): { keywords: Record<string, true>; inverse: EmailPatch } {
  const held = new Map(Object.entries(keywords));
  const inverse: [string, true | null][] = [];
  for (const [path, value] of Object.entries(patch)) {
    const keyword = keywordOf(path);
    inverse.push([path, held.has(keyword) ? true : null]);
    if (value === null) {
      held.delete(keyword);
    } else {
      held.set(keyword, true);
    }
  }
  return { keywords: Object.fromEntries(held), inverse: Object.fromEntries(inverse) };
}

// Whether a patch changes any keyword, given the patch that takes it back.
export function alters(patch: EmailPatch, inverse: EmailPatch): boolean {
  const before = new Map(Object.entries(inverse));
  return Object.entries(patch).some(([path, value]) => before.get(path) !== value);
}

// The keywords with every pending row of the email laid over them, the
// oldest first. Each row comes back with the inverse that fits these
// keywords.
export function replayed(
  keywords: Record<string, true>,
  rows: readonly PendingRow[],
): { keywords: Record<string, true>; rows: PendingRow[] } {
  let current = keywords;
  const fitted = rows.map((row) => {
    const step = patched(current, row.patch);
    current = step.keywords;
    return { ...row, inverse: step.inverse };
  });
  return { keywords: current, rows: fitted };
}

// The keywords as they stood before the pending rows, the newest undone
// first.
export function unpatched(
  keywords: Record<string, true>,
  rows: readonly PendingRow[],
): Record<string, true> {
  return rows.reduceRight((current, row) => patched(current, row.inverse).keywords, keywords);
}
