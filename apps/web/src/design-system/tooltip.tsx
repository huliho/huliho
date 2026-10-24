// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import type { ReactElement } from "react";

import { KeyCaps } from "../commands/key-caps";
import type { Chord } from "../commands/keys";
import styles from "./tooltip.module.css";

// The hint stands this far from its control, on the grid a menu keeps.
const TOOLTIP_OFFSET_PX = 4;

// Hints of one bar: once one has shown, the next opens at once.
export const TooltipGroup = BaseTooltip.Provider;

interface TooltipProps {
  label: string;
  // The keys of the control's command; none for a control without one.
  keys: readonly Chord[];
  // Which side of the control the hint stands on: under it, or over a
  // control at the viewport's foot.
  side?: "top" | "bottom";
  // Which edge of the control the hint lines up with.
  align?: "start" | "end";
  // Whether the hint stands open from the start, as a drawn state does.
  defaultOpen?: boolean;
  // The control, which keeps its own name.
  children: ReactElement;
}

// The label and the keys of a control on the other theme's surface, on
// hover and on focus and never on touch. The control keeps its own name,
// so a screen reader hears the label once.
export function Tooltip(props: TooltipProps) {
  const { label, keys, side = "bottom", align = "start", defaultOpen, children } = props;
  return (
    <BaseTooltip.Root defaultOpen={defaultOpen}>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side={side} align={align} sideOffset={TOOLTIP_OFFSET_PX}>
          <BaseTooltip.Popup className={styles.popup}>
            {label}
            <KeyCaps keys={keys} spoken={false} />
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
