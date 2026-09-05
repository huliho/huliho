// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Field as BaseField } from "@base-ui/react/field";
import { CircleAlert } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";

import styles from "./field.module.css";

interface FieldProps extends Omit<ComponentProps<"input">, "className" | "children"> {
  label: string;
  error?: string | undefined;
  // Options turn the control into a select.
  children?: ReactNode;
}

// Label, control and error line with their aria wiring. Values, checks
// and submission stay with the form.
export function Field({ label, error, children, ...input }: FieldProps) {
  return (
    <BaseField.Root className={styles.field} invalid={error !== undefined}>
      <BaseField.Label className={styles.label}>{label}</BaseField.Label>
      <BaseField.Control
        className={styles.input}
        render={children === undefined ? undefined : <select>{children}</select>}
        {...input}
      />
      {error !== undefined && (
        <BaseField.Error className={styles.error} match role="alert">
          <CircleAlert className={styles.errorIcon} aria-hidden="true" />
          {error}
        </BaseField.Error>
      )}
    </BaseField.Root>
  );
}

// The string a named control holds, empty when there is none.
export function formEntry(data: FormData, name: string): string {
  const value = data.get(name);
  return typeof value === "string" ? value : "";
}

export function focusFormField(form: HTMLFormElement, name: string): void {
  const control = form.elements.namedItem(name);
  if (control instanceof HTMLElement) {
    control.focus();
  }
}
