// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Role, UsersFailureCode } from "@huliho/core";
import type { RefObject } from "react";
import { useRef } from "react";

import { Button } from "../../design-system/button";
import { Dialog, DialogActions } from "../../design-system/dialog";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { CreateUserForm } from "./create-user-form";
import { IssuedSecret } from "./issued-secret";
import type { Flow, Issued, UserFlow } from "./use-user-flow";

interface UserDialogProps {
  locale: Locale;
  // The roles the actor may grant, lowest first.
  roles: Role[];
  flow: UserFlow;
}

type CancelRef = RefObject<HTMLButtonElement | null>;

interface ResetConfirmProps {
  locale: Locale;
  pending: boolean;
  cancel: CancelRef;
  onConfirm: () => void;
  onCancel: () => void;
}

interface DialogText {
  title: string;
  description: string;
}

function issuedText(issued: Issued, locale: Locale): DialogText {
  const deadline = new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(issued.expiresAt);
  const inputs = { name: issued.name, deadline };
  return {
    title: m.users_issued_title({ name: issued.name }, { locale }),
    description:
      issued.reason === "reset"
        ? m.users_issued_note_reset(inputs, { locale })
        : m.users_issued_note(inputs, { locale }),
  };
}

function dialogText(flow: Flow, issued: Issued | null, locale: Locale): DialogText {
  if (issued !== null) {
    return issuedText(issued, locale);
  }
  if (flow.kind === "reset") {
    const inputs = { name: flow.user.name };
    return {
      title: m.users_reset_title(inputs, { locale }),
      description: m.users_reset_explanation(inputs, { locale }),
    };
  }
  return {
    title: m.users_create_title({}, { locale }),
    description: m.users_create_explanation({}, { locale }),
  };
}

// What the dialog says about a refused request; a taken sign-in name is
// said on its field instead.
function failureText(
  flow: Flow,
  failure: UsersFailureCode | null,
  locale: Locale,
): string | undefined {
  if (failure === null) {
    return undefined;
  }
  if (flow.kind === "reset") {
    return m.users_reset_failed({}, { locale });
  }
  return failure === "login_taken" ? undefined : m.users_create_failed({}, { locale });
}

function loginErrorText(failure: UsersFailureCode | null, locale: Locale): string | undefined {
  return failure === "login_taken" ? m.users_error_login_taken({}, { locale }) : undefined;
}

function ResetConfirm({ locale, pending, cancel, onConfirm, onCancel }: ResetConfirmProps) {
  return (
    <DialogActions>
      <Button ref={cancel} held={pending} onClick={onCancel}>
        {m.cancel_action({}, { locale })}
      </Button>
      <Button variant="danger" pending={pending} onClick={onConfirm}>
        {pending ? m.users_resetting({}, { locale }) : m.users_reset({}, { locale })}
      </Button>
    </DialogActions>
  );
}

function DialogContent({ locale, roles, flow, cancel }: UserDialogProps & { cancel: CancelRef }) {
  if (flow.issued !== null) {
    return <IssuedSecret locale={locale} secret={flow.issued.secret} onDone={flow.close} />;
  }
  if (flow.flow?.kind === "reset") {
    return (
      <ResetConfirm
        locale={locale}
        pending={flow.pending}
        cancel={cancel}
        onConfirm={flow.confirmReset}
        onCancel={flow.close}
      />
    );
  }
  return (
    <CreateUserForm
      locale={locale}
      roles={roles}
      pending={flow.pending}
      loginError={loginErrorText(flow.failure, locale)}
      onSubmit={flow.submitCreate}
      onEdit={flow.clearFailure}
      onCancel={flow.close}
    />
  );
}

// One dialog for the whole flow: confirm, the issued password and the
// create form share it, so the step to the password swaps in place.
export function UserDialog({ locale, roles, flow }: UserDialogProps) {
  const cancel = useRef<HTMLButtonElement>(null);
  if (flow.flow === null) {
    return null;
  }
  const { title, description } = dialogText(flow.flow, flow.issued, locale);
  return (
    <Dialog
      open={flow.open}
      onOpenChange={(open) => {
        if (!open && !flow.pending) {
          flow.close();
        }
      }}
      onClosed={flow.settle}
      title={title}
      description={description}
      failure={failureText(flow.flow, flow.failure, locale)}
      initialFocus={flow.flow.kind === "reset" ? cancel : undefined}
    >
      <DialogContent locale={locale} roles={roles} flow={flow} cancel={cancel} />
    </Dialog>
  );
}
