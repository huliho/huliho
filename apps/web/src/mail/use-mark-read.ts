// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, MailCache } from "@huliho/core";
import { useEffect, useRef } from "react";

import { isUnread } from "./thread-messages";

const SEEN_PATH = "keywords/$seen";

// Marks the message read as the card opens, once per opening, when it
// reads unread at that moment; one that turns unread under an open card stays unread.
export function useMarkRead(
  cache: MailCache,
  accountId: string,
  email: EmailHeader,
  open: boolean,
): void {
  const unread = isUnread(email);
  const { id } = email;
  // The message this opening was settled for, marked or not.
  const settled = useRef<string | null>(null);
  useEffect(() => {
    if (!open) {
      settled.current = null;
      return;
    }
    if (settled.current === id) {
      return;
    }
    settled.current = id;
    if (!unread) {
      return;
    }
    cache
      .mutate(accountId, { type: "Email", id, patch: { [SEEN_PATH]: true } })
      .catch((error: unknown) => {
        console.error(
          "mail: marking as read failed",
          error instanceof Error ? error.message : String(error),
        );
      });
  }, [cache, accountId, id, open, unread]);
}
