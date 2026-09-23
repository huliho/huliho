// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expose } from "comlink";

import { Coordinator } from "./coordinator";
import { DexieMailStore, MailDatabase } from "./db";
import { webLocks } from "./locks";
import { CACHE_CHANNEL } from "./messages";

// Every tab of the origin hears the channel; the coordinator gets the
// bound method as a plain function.
const channel = new BroadcastChannel(CACHE_CHANNEL);
const post = channel.postMessage.bind(channel);

const coordinator = new Coordinator({
  store: new DexieMailStore(new MailDatabase()),
  locks: webLocks(navigator.locks),
  post,
});

// A shared worker meets each tab on a connect event; a dedicated worker
// is one tab's own and answers on its global scope.
if ("onconnect" in self) {
  self.addEventListener("connect", (event) => {
    if (event instanceof MessageEvent) {
      for (const port of event.ports) {
        expose(coordinator.api(), port);
      }
    }
  });
} else {
  expose(coordinator.api());
}
