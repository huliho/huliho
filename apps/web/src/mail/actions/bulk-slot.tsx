// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useLayoutEffect, useRef } from "react";

import { ESCAPE } from "../../commands/keys";
import { useRegistered } from "../../commands/use-command";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { useCoarsePointer } from "../../shell/pointer";
import { prefetchMenus } from "../menus-chunk";
import { iconOf } from "./action-icon";
import type { BulkBarProps, BulkEntry, BulkHandle } from "./bulk-handle";
import { BULK_KINDS, useShownActions } from "./kinds";
import type { ActionKind } from "./kinds";
import styles from "./bulk-slot.module.css";

// Under touch the bar keeps what a thumb does to a stack of mail; the
// rest waits behind More actions.
const TOUCH_KINDS: ReadonlySet<ActionKind> = new Set(["archive", "delete", "read"]);

interface BulkSlotProps {
  locale: Locale;
  handle: BulkHandle;
}

function LoadedBulkBar(props: BulkBarProps) {
  const { BulkBar } = use(prefetchMenus());
  return <BulkBar {...props} />;
}

// What the bar draws: the actions the picks can take as the registry
// holds them, so a button stands exactly while its key works; then Close.
function useBarProps({ locale, handle }: BulkSlotProps): BulkBarProps {
  const shown = useShownActions(BULK_KINDS, handle.mailbox, handle.thread, locale);
  const close = useRegistered().findLast((command) => command.id === "selection.close");
  const touch = useCoarsePointer();
  // With nothing picked a press would take nothing, so Close stands alone.
  const entries =
    handle.count === 0
      ? []
      : shown.map((entry): BulkEntry => ({
          kind: entry.kind,
          label: entry.label,
          keys: entry.keys,
          Icon: iconOf(entry.action),
        }));
  return {
    count: {
      text: m.bulk_selected({ count: handle.count }, { locale }),
      figure: new Intl.NumberFormat(locale).format(handle.count),
    },
    inline: touch ? entries.filter((entry) => TOUCH_KINDS.has(entry.kind)) : entries,
    behind: touch ? entries.filter((entry) => !TOUCH_KINDS.has(entry.kind)) : [],
    moreLabel: m.bulk_more({}, { locale }),
    close: {
      label: close?.label ?? m.command_selection({ action: "close" }, { locale }),
      keys: close?.keys ?? [ESCAPE],
    },
    run: handle.run,
    onClose: handle.close,
  };
}

// The bulk bar floats where the new-mail marker does. A bar that leaves
// holding the focus hands it to a row of the grid beside it, which the
// cursor then takes to its own row.
export function BulkSlot(props: BulkSlotProps) {
  const ref = useRef<HTMLDivElement>(null);
  const bar = useBarProps(props);
  useLayoutEffect(() => {
    const slot = ref.current;
    return () => {
      if (slot?.contains(document.activeElement) === true) {
        slot.parentElement
          ?.querySelector<HTMLElement>('[role="row"][tabindex="0"]')
          ?.focus({ preventScroll: true });
      }
    };
  }, []);
  return (
    <div ref={ref} className={styles.slot}>
      <Suspense fallback={null}>
        <LoadedBulkBar {...bar} />
      </Suspense>
    </div>
  );
}
