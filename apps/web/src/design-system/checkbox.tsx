// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Check } from "lucide-react";

import { cx } from "./cx";
import styles from "./checkbox.module.css";

interface CheckboxProps {
  checked: boolean;
  label: string;
  // Told of a press with the event behind it, so a caller can read its keys.
  onCheckedChange: (checked: boolean, event: Event) => void;
  // Whether the box is a tab stop and takes the focus on a press; a box
  // inside a composite widget leaves the focus where it stands.
  focusable?: boolean;
  className?: string | undefined;
}

// A box that ticks, drawn at the icon's size inside a target of the
// density's hit size.
export function Checkbox({
  checked,
  label,
  onCheckedChange,
  focusable = true,
  className,
}: CheckboxProps) {
  return (
    <BaseCheckbox.Root
      checked={checked}
      aria-label={label}
      tabIndex={focusable ? undefined : -1}
      className={cx(styles.root, className)}
      onMouseDown={(event) => {
        if (!focusable) {
          event.preventDefault();
        }
      }}
      onCheckedChange={(next, details) => {
        onCheckedChange(next, details.event);
      }}
    >
      <span className={styles.box}>
        <BaseCheckbox.Indicator className={styles.indicator}>
          <Check className={styles.check} aria-hidden="true" />
        </BaseCheckbox.Indicator>
      </span>
    </BaseCheckbox.Root>
  );
}
