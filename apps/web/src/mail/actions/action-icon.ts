// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailAction } from "@huliho/core";
import { Archive, Mail, MailOpen, ShieldAlert, Star, Trash2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";

// The icon of an action, the same on the row and in the toolbar. A
// flip's icon shows what a press does: the open envelope marks as read,
// the closed one as unread, the filled star takes the star away.
export function iconOf(action: MailAction): LucideIcon {
  switch (action) {
    case "archive":
      return Archive;
    case "delete":
    case "deleteForever":
      return Trash2;
    case "spam":
    case "notSpam":
      return ShieldAlert;
    case "read":
      return MailOpen;
    case "unread":
      return Mail;
    case "star":
    case "unstar":
      return Star;
    default:
      return action satisfies never;
  }
}
