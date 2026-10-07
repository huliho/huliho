// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";
import type { RefObject } from "react";

import type { Chord } from "../commands/keys";
import { useCommand } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

// What a card gets of the inspector: whether its own stands open, what
// opened it and the two ways to change that.
export interface Inspect {
  open: boolean;
  // The element that takes the focus back; null for an opening by command.
  opener: HTMLElement | null;
  onOpen: (opener: HTMLElement) => void;
  onClose: () => void;
}

interface Opened {
  id: string;
  opener: HTMLElement | null;
}

// An open card of the thread, by the id it carries.
const OPEN_CARD = "li[data-message-id][data-expanded]";
// The command reaches the palette alone.
const NO_KEYS: readonly Chord[] = [];

// The card the command inspects: the open card the focus is in, else
// the newest open card.
function inspectTarget(list: HTMLElement | null): string | null {
  if (list === null) {
    return null;
  }
  const focused = document.activeElement?.closest<HTMLElement>(OPEN_CARD) ?? null;
  const target =
    focused !== null && list.contains(focused)
      ? focused
      : [...list.querySelectorAll<HTMLElement>(OPEN_CARD)].at(-1);
  return target?.dataset["messageId"] ?? null;
}

// Which card's inspector stands open, with the command that opens one
// from the palette while any card of the list is open.
export function useInspect(
  listRef: RefObject<HTMLElement | null>,
  locale: Locale,
  enabled: boolean,
) {
  const [opened, setOpened] = useState<Opened | null>(null);
  useCommand(
    enabled
      ? {
          id: "message.inspect",
          label: m.inspector_title({}, { locale }),
          group: "act",
          keys: NO_KEYS,
          run: () => {
            const id = inspectTarget(listRef.current);
            if (id !== null) {
              setOpened({ id, opener: null });
            }
          },
        }
      : null,
  );
  return {
    of: (id: string): Inspect => ({
      open: opened?.id === id,
      opener: opened?.id === id ? opened.opener : null,
      onOpen: (opener) => {
        setOpened({ id, opener });
      },
      onClose: () => {
        setOpened(null);
      },
    }),
  };
}
