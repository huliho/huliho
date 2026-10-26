// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { useNavigate } from "@tanstack/react-router";

import type { Chord } from "../../commands/keys";
import { useCommand } from "../../commands/use-command";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { markedFromMailbox } from "../thread-history";

// `/` opens the search, matched by the key it types on any layout.
export const SEARCH_KEYS: readonly Chord[] = [{ key: "/" }];

// The header's search button wears this, so the focus can find it again.
const SEARCH_BUTTON = "data-search-button";
export const SEARCH_MARK = { [SEARCH_BUTTON]: "" };

// The roles whose own mailbox a search opened there keeps to.
const KEPT_TO = new Set(["junk", "trash"]);

type Place = "button" | "list";

// The element the focus stood on when the search opened, and the place
// that takes the focus back once the mailbox draws it again.
let opener: Element | null = null;
let returning: Place | null = null;

function placeOf(element: Element | null): Place | null {
  if (element?.closest('[role="grid"]') != null) {
    return "list";
  }
  return element?.hasAttribute(SEARCH_BUTTON) === true ? "button" : null;
}

// Closing hands the focus back: to the element itself where it still
// stands, else to the list or the header's button as they mount again.
export function returnFocus(): void {
  const element = opener;
  opener = null;
  returning = null;
  if (element instanceof HTMLElement && element.isConnected) {
    element.focus({ preventScroll: true });
    return;
  }
  returning = placeOf(element);
}

// Whether `place` takes the focus as it mounts; the first to ask spends it.
export function takesReturn(place: Place): boolean {
  if (returning !== place) {
    return false;
  }
  returning = null;
  return true;
}

function paramsFrom(mailbox: Mailbox | undefined): { box?: string; in?: string } {
  if (mailbox === undefined) {
    return {};
  }
  const kept = mailbox.role !== null && KEPT_TO.has(mailbox.role);
  return kept ? { box: mailbox.id, in: mailbox.id } : { box: mailbox.id };
}

// Registers `search.open` while the shell shows a mailbox and answers
// it: the search of the account opened from the mailbox shown, one entry
// pushed. While a search stands, its pane answers the command instead.
export function useSearchCommand(
  accountId: string,
  mailbox: Mailbox | undefined,
  locale: Locale,
  searching: boolean,
): void {
  const navigate = useNavigate();
  const open = (): void => {
    opener = document.activeElement;
    void navigate({
      to: "/mail/$accountId/search",
      params: { accountId },
      search: paramsFrom(mailbox),
      state: markedFromMailbox,
    });
  };
  useCommand(
    searching
      ? null
      : {
          id: "search.open",
          label: m.command_search({}, { locale }),
          group: "app",
          keys: SEARCH_KEYS,
          run: open,
        },
  );
}
