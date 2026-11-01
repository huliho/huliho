// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { setMailbox } from "@huliho/core";
import type { JmapClient, MailStore } from "@huliho/core";

import type { CacheMessage } from "./messages";
import { attempt } from "./outcome";
import type { CacheApi } from "./surface";
import { moved } from "./writes";

// What the folder writes ask of the coordinator: an account's client,
// its lock, the channel to the tabs and the lists the tabs watch.
export interface FolderHost {
  client(accountId: string): JmapClient;
  locked<Value>(accountId: string, run: (store: MailStore) => Promise<Value>): Promise<Value>;
  post(message: CacheMessage): void;
  watched(accountId: string): string[];
}

// The account's folder writes and limits. A write runs under the
// account's lock, so no poll lands rows under it; every tab hears what
// it moved. The limits come with the session the client holds.
export function folderCalls(host: FolderHost): Pick<CacheApi, "setMailbox" | "mailboxLimits"> {
  return {
    setMailbox: (accountId, write) =>
      attempt(async () => {
        const outcome = await host.locked(accountId, (store) =>
          setMailbox(host.client(accountId), store, write, host.watched(accountId)),
        );
        if (moved(outcome.changes)) {
          host.post({ kind: "changed", accountId, ...outcome.changes });
        }
        return outcome;
      }),
    mailboxLimits: (accountId) =>
      attempt(async () => (await host.client(accountId).session()).mailboxLimits),
  };
}
