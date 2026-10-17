// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLocation, useNavigate, useRouter } from "@tanstack/react-router";

import { openedFromMailbox } from "./thread-history";

// Closing goes back over the entry the list pushed, so the history
// reads as it did before the open; a thread reached by its address
// leaves it in place instead.
export function useCloseThread(accountId: string, mailboxId: string | undefined): () => void {
  const navigate = useNavigate();
  const router = useRouter();
  const fromMailbox = useLocation({ select: (location) => openedFromMailbox(location.state) });
  return () => {
    if (mailboxId === undefined) {
      return;
    }
    if (fromMailbox) {
      router.history.back();
      return;
    }
    void navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId, mailboxId },
      replace: true,
    });
  };
}
