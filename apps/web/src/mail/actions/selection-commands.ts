// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useEffect, useRef } from "react";
import type { RefObject } from "react";

import { ESCAPE } from "../../commands/keys";
import type { Chord } from "../../commands/keys";
import { useCommand } from "../../commands/use-command";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { ListCursor } from "../list-cursor";
import { rowAt } from "../use-thread-pages";
import { announce } from "./announce";
import type { Selection } from "./selection";
import { lendSelection } from "./selection-lend";

const TOGGLE_KEYS: readonly Chord[] = [{ key: "x" }];
const CLOSE_KEYS: readonly Chord[] = [ESCAPE];

interface SelectionInput {
  locale: Locale;
  rows: ReadonlyMap<number, ListRow[]>;
  cursor: ListCursor;
  selection: Selection;
  regionRef: RefObject<HTMLElement | null>;
  // Whether the mailbox offers an action the picks could take.
  pickable: boolean;
}

// The selection's keys: `x` toggles the cursor's row while the mailbox
// lets threads be picked, Escape closes a standing mode. The pane
// header's Select and the thread's Escape reach the mode through the
// lend; the status region says the count once the mode opens.
export function useSelectionCommands(input: SelectionInput): void {
  const { locale, rows, cursor, selection, regionRef, pickable } = input;
  const row = pickable ? rowAt(rows, cursor.active) : undefined;
  useCommand(
    row === undefined
      ? null
      : {
          id: "selection.toggle",
          label: m.command_selection(
            { action: selection.has(row.threadId) ? "deselect" : "select" },
            { locale },
          ),
          group: "act",
          keys: TOGGLE_KEYS,
          run: () => {
            selection.toggle(row);
          },
        },
  );
  useCommand(
    selection.open
      ? {
          id: "selection.close",
          label: m.command_selection({ action: "close" }, { locale }),
          group: "act",
          keys: CLOSE_KEYS,
          run: selection.clear,
        }
      : null,
  );
  useEffect(() =>
    lendSelection({
      start: () => {
        if (pickable) {
          selection.start();
        }
      },
      close: () => {
        if (!selection.open) {
          return false;
        }
        selection.clear();
        return true;
      },
    }),
  );
  useAnnouncedMode(selection, regionRef, locale);
}

// The status region says the count once the mode opens, as the bar
// appears over the rows.
function useAnnouncedMode(
  selection: Selection,
  regionRef: RefObject<HTMLElement | null>,
  locale: Locale,
): void {
  const wasOpen = useRef(false);
  const { open, count } = selection;
  useEffect(() => {
    if (open && !wasOpen.current) {
      announce(regionRef.current, m.bulk_selected({ count }, { locale }));
    }
    wasOpen.current = open;
  }, [open, count, regionRef, locale]);
}
