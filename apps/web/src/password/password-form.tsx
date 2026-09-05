// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { CredentialFailureCode, PasswordChangeInput } from "@huliho/core";
import { PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS, fitsPasswordWindow } from "@huliho/core";
import type { Ref, SubmitEvent } from "react";
import { useState } from "react";

import { CredentialNotice, retryLabel } from "../auth/credential-notice";
import { Button } from "../design-system/button";
import { Field } from "../design-system/field";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./password-form.module.css";

// A change asks for the current password; the forced step already holds proof.
type PasswordFormMode = "change" | "forced";

type Issue = "window" | "mismatch";

export interface PasswordFormProps {
  locale: Locale;
  mode: PasswordFormMode;
  pending: boolean;
  failure: CredentialFailureCode | null;
  retryRemaining: number | null;
  onSubmit: (input: PasswordChangeInput) => void;
  // The submit sits at the start of its card unless the parent styles it.
  submitClassName?: string | undefined;
  // Reaches the form element, so a parent clears the fields through its own reset.
  ref?: Ref<HTMLFormElement> | undefined;
}

interface PasswordFieldsProps {
  locale: Locale;
  mode: PasswordFormMode;
  held: boolean;
  failure: CredentialFailureCode | null;
  issue: Issue | null;
  onEdit: () => void;
}

function entry(data: FormData, name: string): string {
  const value = data.get(name);
  return typeof value === "string" ? value : "";
}

// The first thing wrong with what was typed; null when it can go out.
function issueOf(next: string, repeat: string): Issue | null {
  if (!fitsPasswordWindow(next)) {
    return "window";
  }
  return next === repeat ? null : "mismatch";
}

function focusField(form: HTMLFormElement, name: string): void {
  const input = form.elements.namedItem(name);
  if (input instanceof HTMLInputElement) {
    input.focus();
  }
}

function submitLabel(locale: Locale, mode: PasswordFormMode, pending: boolean): string {
  if (pending) {
    return m.password_saving({}, { locale });
  }
  return mode === "change"
    ? m.password_save({}, { locale })
    : m.password_save_continue({}, { locale });
}

function PasswordFields({ locale, mode, held, failure, issue, onEdit }: PasswordFieldsProps) {
  const windowMessage = m.password_error_window(
    { min: PASSWORD_MIN_CHARS, max: PASSWORD_MAX_CHARS },
    { locale },
  );
  return (
    <>
      {mode === "change" && (
        <Field
          name="current"
          label={m.password_current_label({}, { locale })}
          type="password"
          autoComplete="current-password"
          required
          readOnly={held}
          error={
            failure === "invalid_credentials" ? m.password_error_current({}, { locale }) : undefined
          }
        />
      )}
      <Field
        name="new"
        label={m.password_new_label({}, { locale })}
        type="password"
        autoComplete="new-password"
        required
        readOnly={held}
        onChange={onEdit}
        error={issue === "window" ? windowMessage : undefined}
      />
      <Field
        name="repeat"
        label={m.password_repeat_label({}, { locale })}
        type="password"
        autoComplete="new-password"
        required
        readOnly={held}
        onChange={onEdit}
        error={issue === "mismatch" ? m.password_error_mismatch({}, { locale }) : undefined}
      />
    </>
  );
}

// Uncontrolled on purpose: nothing keeps a password in state. A native
// reset clears the fields while focus stays where it was.
export function PasswordForm({
  locale,
  mode,
  pending,
  failure,
  retryRemaining,
  onSubmit,
  submitClassName,
  ref,
}: PasswordFormProps) {
  const [issue, setIssue] = useState<Issue | null>(null);
  const held = pending || failure === "rate_limited";
  const submit = (event: SubmitEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (held) {
      return;
    }
    const data = new FormData(event.currentTarget);
    const next = entry(data, "new");
    const found = issueOf(next, entry(data, "repeat"));
    setIssue(found);
    if (found !== null) {
      focusField(event.currentTarget, found === "window" ? "new" : "repeat");
      return;
    }
    onSubmit(mode === "change" ? { current: entry(data, "current"), new: next } : { new: next });
  };
  const label = retryLabel(locale, retryRemaining) ?? submitLabel(locale, mode, pending);
  return (
    <form ref={ref} className={styles.form} onSubmit={submit}>
      <CredentialNotice locale={locale} failure={failure} />
      <PasswordFields
        locale={locale}
        mode={mode}
        held={held}
        failure={failure}
        issue={issue}
        onEdit={() => {
          setIssue(null);
        }}
      />
      <Button
        variant="primary"
        className={submitClassName ?? styles.save}
        type="submit"
        held={held}
        pending={pending}
      >
        {label}
      </Button>
    </form>
  );
}
