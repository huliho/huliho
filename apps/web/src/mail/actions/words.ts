// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ChangeVerb, MailAction } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// The actions that move threads and say so in a toast.
const SAID: ReadonlySet<MailAction> = new Set([
  "archive",
  "delete",
  "deleteForever",
  "spam",
  "notSpam",
]);

// The action's name in the palette, the overlay and a label.
export function actionLabel(action: MailAction, locale: Locale): string {
  return m.command_thread({ action }, { locale });
}

// What the toast says once an action moved `count` threads; null for
// the flips, which show no toast.
export function doneSentence(action: MailAction, count: number, locale: Locale): string | null {
  return SAID.has(action) ? m.action_done({ action, count }, { locale }) : null;
}

// What the toast says once the server refused a change and the rows
// gave it back.
export function refusedSentence(verb: ChangeVerb, locale: Locale): string {
  return m.refused_change({ verb }, { locale });
}
