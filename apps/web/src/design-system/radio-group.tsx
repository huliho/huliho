// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Radio } from "@base-ui/react/radio";
import { RadioGroup as BaseRadioGroup } from "@base-ui/react/radio-group";

import styles from "./radio-group.module.css";

export interface RadioOption<T extends string> {
  value: T;
  label: string;
}

interface RadioGroupProps<T extends string> {
  // The id of the element that names the group.
  labelledBy: string;
  // The id of a sentence that explains the choice.
  describedBy?: string | undefined;
  options: readonly RadioOption<T>[];
  value: T;
  onChange: (value: T) => void;
}

// A choice among a few words, drawn as one control with a segment per
// word. Arrow keys move the choice; Space or a click picks a segment.
export function RadioGroup<T extends string>({
  labelledBy,
  describedBy,
  options,
  value,
  onChange,
}: RadioGroupProps<T>) {
  return (
    <BaseRadioGroup
      className={styles.group}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      value={value}
      onValueChange={(next) => {
        onChange(next);
      }}
    >
      {options.map((option) => (
        <Radio.Root key={option.value} value={option.value} className={styles.segment}>
          {option.label}
        </Radio.Root>
      ))}
    </BaseRadioGroup>
  );
}
