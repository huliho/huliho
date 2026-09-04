// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Field as BaseField } from "@base-ui/react/field";
import { CircleAlert } from "lucide-react";
import type { ComponentProps } from "react";

import styles from "./field.module.css";

interface FieldProps extends Omit<ComponentProps<"input">, "className" | "children"> {
  label: string;
  error?: string | undefined;
}

// Label, control and error line with their aria wiring. Values, checks
// and submission stay with the form.
export function Field({ label, error, ...input }: FieldProps) {
  return (
    <BaseField.Root className={styles.field} invalid={error !== undefined}>
      <BaseField.Label className={styles.label}>{label}</BaseField.Label>
      <BaseField.Control className={styles.input} {...input} />
      {error !== undefined && (
        <BaseField.Error className={styles.error} match role="alert">
          <CircleAlert className={styles.errorIcon} aria-hidden="true" />
          {error}
        </BaseField.Error>
      )}
    </BaseField.Root>
  );
}
