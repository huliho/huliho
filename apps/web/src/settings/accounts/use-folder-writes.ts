// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, MailboxWrite } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { toastManager } from "../../design-system/toast";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// The one write on its way: which control sent it and the value that
// control shows meanwhile.
export interface PendingWrite {
  control: string;
  value: string;
}

export interface FolderWrites {
  pending: PendingWrite | null;
  // Sends at once.
  send: (control: string, value: string, write: MailboxWrite, done?: string) => void;
  // Sends once the control has stood still; null where nothing would change.
  settle: (control: string, value: string, write: MailboxWrite | null) => void;
}

// On some systems a key walks a closed select through its options, each
// step a change, so a pick goes out once the select has stood still.
export const SETTLE_MS = 400;

// The pick that waits for its select to stand still.
interface Settling {
  send: () => void;
  timer: ReturnType<typeof setTimeout>;
}

// Whether the server wrote every object. A write that never reached it
// leaves its cause in the console.
async function landed(cache: MailCache, accountId: string, write: MailboxWrite): Promise<boolean> {
  try {
    const outcome = await cache.setMailbox(accountId, write);
    return Object.keys(outcome.notUpdated).length === 0;
  } catch (error) {
    console.error(
      "settings: a folder write failed",
      error instanceof Error ? error.message : String(error),
    );
    return false;
  }
}

interface WriteWords {
  cache: MailCache;
  accountId: string;
  locale: Locale;
  done: string | undefined;
}

// Sends one write and says in a toast how it went: `done` where it landed
// and one is given, why not in the danger tone.
async function written(
  write: MailboxWrite,
  { cache, accountId, locale, done }: WriteWords,
): Promise<void> {
  if (!navigator.onLine || !(await landed(cache, accountId, write))) {
    const reason = navigator.onLine ? "server" : "offline";
    toastManager.add({
      description: m.folder_refused({ reason }, { locale }),
      data: { tone: "danger" },
    });
  } else if (done !== undefined) {
    toastManager.add({ description: done });
  }
}

// The screen's writes, one at a time; a control pressed meanwhile stays as
// it is. The value stands until the tree is read again, so a control never
// shows the old mailbox between the answer and the read. A refusal says so
// in a danger toast and the control falls back to what the server holds. A
// pick still waiting goes out when the screen leaves.
export function useFolderWrites(cache: MailCache, accountId: string, locale: Locale): FolderWrites {
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<PendingWrite | null>(null);
  const settling = useRef<Settling | null>(null);
  useEffect(
    () => () => {
      settling.current?.send();
    },
    [],
  );
  const start = (write: MailboxWrite, done?: string): void => {
    void written(write, { cache, accountId, locale, done })
      .then(() => queryClient.invalidateQueries({ queryKey: queryKeys.mailboxes(accountId) }))
      .finally(() => {
        setPending(null);
      });
  };
  return {
    pending,
    send: (control, value, write, done) => {
      if (pending === null) {
        setPending({ control, value });
        start(write, done);
      }
    },
    settle: (control, value, write) => {
      const waiting = settling.current;
      if (pending !== null && (waiting === null || pending.control !== control)) {
        return;
      }
      if (waiting !== null) {
        clearTimeout(waiting.timer);
        settling.current = null;
      }
      if (write === null) {
        setPending(null);
        return;
      }
      setPending({ control, value });
      const send = (): void => {
        if (settling.current !== null) {
          clearTimeout(settling.current.timer);
          settling.current = null;
          start(write);
        }
      };
      settling.current = { send, timer: setTimeout(send, SETTLE_MS) };
    },
  };
}
