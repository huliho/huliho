// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailAction } from "@huliho/core";
import { Archive, FolderInput, Mail, MailOpen, ShieldAlert, Star, Trash2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";

type Flip = Extract<MailAction, "read" | "unread" | "star" | "unstar">;

// A flip's icon shows what a press does: the open envelope marks as
// read, the closed one as unread, the filled star takes the star away.
function flipIcon(action: Flip): LucideIcon {
  switch (action) {
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

// The icon of each action, the same on the row and in the toolbar.
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
    case "move":
      return FolderInput;
    default:
      return flipIcon(action);
  }
}
