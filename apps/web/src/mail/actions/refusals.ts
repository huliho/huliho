// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { RefusedMessage } from "../../cache/client";
import { toastManager } from "../../design-system/toast";
import type { Locale } from "../../paraglide/runtime.js";
import { closeToastsOf } from "./action-toast";
import { refusedSentence } from "./words";

// The server refused changes and the rows gave them back: the toasts of
// the actions behind them leave in every tab, and the tab that `tells`
// says once per verb that the mail is safe.
export function showRefused(message: RefusedMessage, locale: Locale, tells: boolean): void {
  closeToastsOf(message.emailIds);
  if (!tells) {
    return;
  }
  for (const verb of message.verbs) {
    toastManager.add({ description: refusedSentence(verb, locale), data: { tone: "danger" } });
  }
}
