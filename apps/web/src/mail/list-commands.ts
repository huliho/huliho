// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Chord } from "../commands/keys";
import { useCommand } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { REVEAL_KEYS } from "./thread-grid";

// The keys that move the cursor from anywhere and the one that opens
// its row, as Enter does in the grid.
const NEXT_KEYS: readonly Chord[] = [{ key: "j" }];
const PREVIOUS_KEYS: readonly Chord[] = [{ key: "k" }];
const OPEN_KEYS: readonly Chord[] = [{ key: "o" }];

interface Commands {
  locale: Locale;
  rowCount: number;
  pending: number;
  active: number;
  goTo: (index: number) => void;
  open: () => void;
  reveal: () => void;
}

// j and k move the cursor from anywhere on the screen and o opens its
// row; the dot key brings new mail in while some waits.
export function useListCommands(commands: Commands): void {
  const { locale, rowCount, pending, active, goTo, open, reveal } = commands;
  const next = (): void => {
    goTo(active + 1);
  };
  const previous = (): void => {
    goTo(active - 1);
  };
  const listed = rowCount > 0;
  useCommand(
    listed
      ? {
          id: "list.next",
          label: m.command_list_next({}, { locale }),
          group: "navigate",
          keys: NEXT_KEYS,
          run: next,
        }
      : null,
  );
  useCommand(
    listed
      ? {
          id: "list.previous",
          label: m.command_list_previous({}, { locale }),
          group: "navigate",
          keys: PREVIOUS_KEYS,
          run: previous,
        }
      : null,
  );
  useCommand(
    listed
      ? {
          id: "list.open",
          label: m.command_list_open({}, { locale }),
          group: "navigate",
          keys: OPEN_KEYS,
          run: open,
        }
      : null,
  );
  useCommand(
    pending > 0
      ? {
          id: "list.reveal",
          label: m.command_list_reveal({}, { locale }),
          group: "act",
          keys: REVEAL_KEYS,
          run: reveal,
        }
      : null,
  );
}
