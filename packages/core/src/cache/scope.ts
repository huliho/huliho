// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MemberState } from "./store";

// The mailboxes a search covers: a single one, all but some of them or
// every one.
export type Scope =
  { kind: "one"; mailboxId: string } | { kind: "except"; mailboxIds: string[] } | { kind: "all" };

// Whether a scope covers one mailbox.
export function covers(scope: Scope, mailboxId: string): boolean {
  switch (scope.kind) {
    case "one":
      return scope.mailboxId === mailboxId;
    case "except":
      return !scope.mailboxIds.includes(mailboxId);
    default:
      return true;
  }
}

// Whether an email stands in a mailbox the scope covers, as
// `inMailboxOtherThan` reads it (RFC 8621 section 4.4.1).
export function inScope(state: MemberState, scope: Scope): boolean {
  return Object.keys(state.mailboxIds).some((id) => covers(scope, id));
}
