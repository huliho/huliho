// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLocation, useNavigate, useRouter } from "@tanstack/react-router";

import { openedFromMailbox, searchDepthOf } from "./thread-history";

// Closing goes back over the entry the list pushed, so the history
// reads as it did before the open; a thread reached by its address
// gives way to its list instead: the mailbox, or the results of the
// search it stands in, whose params stay.
export function useCloseThread(accountId: string, mailboxId: string | undefined): () => void {
  const navigate = useNavigate();
  const router = useRouter();
  const pushed = useLocation({
    select: (location) =>
      mailboxId === undefined
        ? searchDepthOf(location.state) > 0
        : openedFromMailbox(location.state),
  });
  return () => {
    if (pushed) {
      router.history.back();
    } else if (mailboxId === undefined) {
      void navigate({ to: "..", search: ({ at: _at, ...kept }) => kept, replace: true });
    } else {
      void navigate({
        to: "/mail/$accountId/$mailboxId",
        params: { accountId, mailboxId },
        replace: true,
      });
    }
  };
}
