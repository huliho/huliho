// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { NewUser, Role } from "@huliho/core";
import { USER_NAME_MAX_CHARS, fitsLogin } from "@huliho/core";
import type { SubmitEvent } from "react";
import { useState } from "react";

import { Button } from "../../design-system/button";
import { DialogActions } from "../../design-system/dialog";
import { Field, focusFormField, formEntry } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { roleLabel } from "./role-badge";
import styles from "./create-user-form.module.css";

export interface CreateUserFormProps {
  locale: Locale;
  // The roles the actor may grant, lowest first; the first is preselected.
  roles: Role[];
  pending: boolean;
  // The server's word on the sign-in name, shown on its field.
  loginError: string | undefined;
  onSubmit: (user: NewUser) => void;
  onEdit: () => void;
  onCancel: () => void;
}

// The field whose value cannot go out as typed.
type Issue = "name" | "login";

interface UserFieldsProps {
  locale: Locale;
  roles: Role[];
  pending: boolean;
  nameIssue: string | undefined;
  loginIssue: string | undefined;
  onEdit: () => void;
}

// The first thing wrong with what was typed; null when it can go out.
function issueOf(name: string, login: string): Issue | null {
  if (name === "") {
    return "name";
  }
  return fitsLogin(login) ? null : "login";
}

function UserFields({ locale, roles, pending, nameIssue, loginIssue, onEdit }: UserFieldsProps) {
  return (
    <>
      <Field
        name="name"
        label={m.users_name({}, { locale })}
        type="text"
        autoComplete="off"
        required
        maxLength={USER_NAME_MAX_CHARS}
        readOnly={pending}
        error={nameIssue}
        onChange={onEdit}
      />
      <Field
        name="login"
        label={m.users_login({}, { locale })}
        type="text"
        autoComplete="off"
        required
        readOnly={pending}
        error={loginIssue}
        onChange={onEdit}
      />
      <Field name="role" label={m.users_role({}, { locale })} defaultValue={roles[0]}>
        {roles.map((role) => (
          <option key={role} value={role}>
            {roleLabel(role, locale)}
          </option>
        ))}
      </Field>
    </>
  );
}

// Uncontrolled: the fields are read on submit. A blank name or a sign-in
// name the server would refuse for its shape never goes out.
export function CreateUserForm({
  locale,
  roles,
  pending,
  loginError,
  onSubmit,
  onEdit,
  onCancel,
}: CreateUserFormProps) {
  const [issue, setIssue] = useState<Issue | null>(null);
  const submit = (event: SubmitEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (pending) {
      return;
    }
    const data = new FormData(event.currentTarget);
    const name = formEntry(data, "name").trim();
    const login = formEntry(data, "login");
    const found = issueOf(name, login);
    setIssue(found);
    if (found !== null) {
      focusFormField(event.currentTarget, found);
      return;
    }
    const chosen = formEntry(data, "role");
    onSubmit({ name, login, role: roles.find((role) => role === chosen) ?? "member" });
  };
  return (
    <form className={styles.form} onSubmit={submit}>
      <UserFields
        locale={locale}
        roles={roles}
        pending={pending}
        nameIssue={issue === "name" ? m.users_error_name_blank({}, { locale }) : undefined}
        loginIssue={issue === "login" ? m.users_error_login_shape({}, { locale }) : loginError}
        onEdit={() => {
          setIssue(null);
          onEdit();
        }}
      />
      <DialogActions>
        <Button type="button" held={pending} onClick={onCancel}>
          {m.cancel_action({}, { locale })}
        </Button>
        <Button variant="primary" type="submit" pending={pending}>
          {pending ? m.users_creating({}, { locale }) : m.users_create_submit({}, { locale })}
        </Button>
      </DialogActions>
    </form>
  );
}
