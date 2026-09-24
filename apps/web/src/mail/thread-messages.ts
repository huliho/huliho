// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, ThreadDetail } from "@huliho/core";

// The collapsed cards kept in sight above the first open one; anything
// older waits behind the button.
const COLLAPSED_IN_SIGHT = 2;

const SEEN = "$seen";

// One message of an open thread as the pane first draws it.
export interface PlannedMessage {
  email: EmailHeader;
  unread: boolean;
  // Open with its recipients and its text; a collapsed card is one line.
  expanded: boolean;
  // Behind "Show older messages" until the user asks.
  older: boolean;
}

export interface ThreadPlan {
  messages: PlannedMessage[];
  olderCount: number;
}

// The thread's messages in the server's order, oldest first, for the
// headers the detail holds.
export function messagesOf(detail: ThreadDetail): EmailHeader[] {
  const emails = new Map(Object.entries(detail.emails));
  return detail.thread.emailIds.flatMap((id) => {
    const email = emails.get(id);
    return email === undefined ? [] : [email];
  });
}

function isUnread(email: EmailHeader): boolean {
  return !(SEEN in email.keywords);
}

// The newest message and every unread one open; the two before the
// first open one stay in sight, collapsed; every message older than
// those waits behind the button.
export function planMessages(messages: readonly EmailHeader[]): ThreadPlan {
  const last = messages.length - 1;
  const firstOpen = messages.findIndex((email, index) => index === last || isUnread(email));
  const keepFrom = Math.max(0, firstOpen - COLLAPSED_IN_SIGHT);
  return {
    messages: messages.map((email, index) => ({
      email,
      unread: isUnread(email),
      expanded: index === last || isUnread(email),
      older: index < keepFrom,
    })),
    olderCount: keepFrom,
  };
}

// The thread's subject: the one its first message carries.
export function subjectOf(messages: readonly EmailHeader[]): string | null {
  const first = messages[0];
  if (first === undefined) {
    return null;
  }
  return first.subject === null || first.subject.trim() === "" ? null : first.subject;
}
