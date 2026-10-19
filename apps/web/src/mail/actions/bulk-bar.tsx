// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Toolbar } from "@base-ui/react/toolbar";
import { Ellipsis, X } from "lucide-react";
import { useRef } from "react";

import { MenuItem, MenuPopup, MenuRoot, MenuTrigger } from "../../design-system/menu";
import { Tooltip, TooltipGroup } from "../../design-system/tooltip";
import type { BulkBarProps, BulkEntry } from "./bulk-handle";
import type { ActionKind } from "./kinds";
import styles from "./bulk-bar.module.css";

// The count as its sentence words it, the figure in mono wherever the
// sentence puts it. The bar's own name says it to a screen reader.
function Count({ text, figure }: BulkBarProps["count"]) {
  const at = text.indexOf(figure);
  return (
    <span className={styles.count} aria-hidden="true">
      {at === -1 ? (
        text
      ) : (
        <>
          {text.slice(0, at)}
          <span className={styles.figure}>{figure}</span>
          {text.slice(at + figure.length)}
        </>
      )}
    </span>
  );
}

function ActionButton({ entry, onPress }: { entry: BulkEntry; onPress: () => void }) {
  const { Icon } = entry;
  return (
    <Tooltip label={entry.label} keys={entry.keys}>
      <Toolbar.Button className={styles.button} aria-label={entry.label} onClick={onPress}>
        <Icon className={styles.icon} aria-hidden="true" />
      </Toolbar.Button>
    </Tooltip>
  );
}

interface MoreProps {
  label: string;
  entries: readonly BulkEntry[];
  run: BulkBarProps["run"];
}

// An item's action waits until the menu has closed, when the focus
// returns to the trigger in the bar, so a bar that leaves after the
// action hands the focus to a row, as it does after its own buttons.
function More({ label, entries, run }: MoreProps) {
  const pending = useRef<ActionKind | null>(null);
  if (entries.length === 0) {
    return null;
  }
  return (
    <MenuRoot
      onOpenChangeComplete={(open) => {
        const kind = pending.current;
        if (!open && kind !== null) {
          pending.current = null;
          run(kind);
        }
      }}
    >
      <MenuTrigger render={<Toolbar.Button className={styles.button} aria-label={label} />}>
        <Ellipsis className={styles.icon} aria-hidden="true" />
      </MenuTrigger>
      <MenuPopup>
        {entries.map((entry) => (
          <MenuItem
            key={entry.kind}
            onClick={() => {
              pending.current = entry.kind;
            }}
          >
            {entry.label}
          </MenuItem>
        ))}
      </MenuPopup>
    </MenuRoot>
  );
}

// The bar over the first rows while a selection stands: the count, then
// the actions the picks can take with a tooltip each, then Close.
export function BulkBar(props: BulkBarProps) {
  const { count, inline, behind, close, run } = props;
  return (
    <TooltipGroup>
      <Toolbar.Root className={styles.pill} aria-label={count.text}>
        <Count {...count} />
        {inline.map((entry) => (
          <ActionButton
            key={entry.kind}
            entry={entry}
            onPress={() => {
              run(entry.kind);
            }}
          />
        ))}
        <More label={props.moreLabel} entries={behind} run={run} />
        <Tooltip label={close.label} keys={close.keys} align="end">
          <Toolbar.Button
            className={styles.button}
            aria-label={close.label}
            onClick={props.onClose}
          >
            <X className={styles.icon} aria-hidden="true" />
          </Toolbar.Button>
        </Tooltip>
      </Toolbar.Root>
    </TooltipGroup>
  );
}
