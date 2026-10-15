// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, Undo } from "@huliho/core";

import { toastManager } from "../../design-system/toast";
import { trackPending } from "../../undo/pending";

// The toasts of actions that still stand, by the emails they changed
// that the server has not refused. A refusal of the last of them takes
// the toast away; one of some leaves it, so Undo takes back the rest.
const standing = new Map<string, Set<string>>();

export function closeToastsOf(emailIds: readonly string[]): void {
  for (const [id, emails] of standing) {
    const before = emails.size;
    for (const emailId of emailIds) {
      emails.delete(emailId);
    }
    if (before > 0 && emails.size === 0) {
      toastManager.close(id);
    }
  }
}

function logged(what: string): (error: unknown) => void {
  return (error) => {
    console.error(`mail: ${what} failed`, error instanceof Error ? error.message : String(error));
  };
}

interface ActionToast {
  cache: MailCache;
  accountId: string;
  text: string;
  undo: Undo;
  emailIds: readonly string[];
}

// One action's toast. Undo sends the changes that take it back and takes
// the held destroys back; once the toast ends the held destroys go out,
// and a page that goes away first lets them go as well.
export function showActionToast({ cache, accountId, text, undo, emailIds }: ActionToast): void {
  const { held, inverses } = undo;
  let toastId = "";
  let settled = false;
  let untrack: (() => void) | undefined;
  const settle = (): boolean => {
    if (settled) {
      return false;
    }
    settled = true;
    untrack?.();
    standing.delete(toastId);
    return true;
  };
  const release = (): void => {
    if (held.length > 0) {
      cache.release(accountId, held).catch(logged("releasing a delete forever"));
    }
  };
  const takeBack = (): void => {
    if (!settle()) {
      return;
    }
    toastManager.close(toastId);
    if (held.length > 0) {
      cache.drop(accountId, held).catch(logged("undoing a delete forever"));
    }
    if (inverses.length > 0) {
      cache.mutateMany(accountId, inverses).catch(logged("undoing an action"));
    }
  };
  if (held.length > 0) {
    untrack = trackPending({
      flush: () => {
        if (settle()) {
          toastManager.close(toastId);
          release();
        }
      },
      reapply: () => undefined,
    });
  }
  toastId = toastManager.add({
    description: text,
    data: { undo: takeBack },
    actionProps: { onClick: takeBack },
    onClose: () => {
      if (settle()) {
        release();
      }
    },
  });
  standing.set(toastId, new Set(emailIds));
}
