// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useRef, useState } from "react";
import type { FocusEvent, RefObject, SubmitEvent } from "react";

import type { Button } from "../../design-system/button";
import type { Dialog, DialogActions } from "../../design-system/dialog";
import type { Field } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { Asked, ConfirmAsk, FolderRefusal, NameAsk } from "./folder-asks";
import styles from "./folder-dialogs.module.css";

// What the shell hands in, so this chunk pulls no module of the page's
// start into a chunk the start shares: the design system's parts and
// the ask's own close, which `settle` follows once the fade has ended.
interface DialogParts {
  Dialog: typeof Dialog;
  DialogActions: typeof DialogActions;
  Field: typeof Field;
  Button: typeof Button;
  close: () => void;
  settle: () => void;
}

const NAME_FIELD = "name";

const encoder = new TextEncoder();

// The first thing wrong with a name before it goes out: none typed, more
// octets than the account takes or the name of a folder beside it.
function nameIssue(name: string, ask: NameAsk): FolderRefusal | null {
  if (name === "") {
    return "empty";
  }
  if (encoder.encode(name).length > ask.limit) {
    return "long";
  }
  return ask.taken.some((taken) => taken.normalize("NFC") === name) ? "taken" : null;
}

// The field's own line says what is wrong with the name as typed.
function fieldWords(refusal: FolderRefusal | null, locale: Locale): string | undefined {
  return refusal === "empty" || refusal === "long" || refusal === "taken"
    ? m.folder_name_refused({ reason: refusal }, { locale })
    : undefined;
}

// The dialog's failure line says what the server or the connection said.
function failureWords(refusal: FolderRefusal | null, locale: Locale): string | undefined {
  if (refusal === null || fieldWords(refusal, locale) !== undefined) {
    return undefined;
  }
  return refusal === "name"
    ? m.folder_name_refused({ reason: refusal }, { locale })
    : m.folder_refused({ reason: refusal }, { locale });
}

// The name as typed, trimmed and in NFC, as the server keeps it.
function typedName(form: HTMLFormElement): string {
  const value = new FormData(form).get(NAME_FIELD);
  return typeof value === "string" ? value.trim().normalize("NFC") : "";
}

// A submit sends the name once nothing is wrong with it; a refusal,
// found here or answered by the server, stands until the text changes.
function useNameSubmit(
  ask: NameAsk,
  inputRef: RefObject<HTMLInputElement | null>,
  close: () => void,
) {
  const [pending, setPending] = useState(false);
  const [refusal, setRefusal] = useState<FolderRefusal | null>(null);
  // The field takes the focus with the refusal, so the name is edited
  // where its line reads, whichever control sent it.
  const refuse = (issue: FolderRefusal): void => {
    setRefusal(issue);
    inputRef.current?.focus();
  };
  const send = async (name: string): Promise<void> => {
    setPending(true);
    setRefusal(null);
    const answer = await ask.write(name);
    setPending(false);
    if (answer === null) {
      close();
    } else {
      refuse(answer);
    }
  };
  const submit = (event: SubmitEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (pending) {
      return;
    }
    const name = typedName(event.currentTarget);
    const issue = navigator.onLine ? nameIssue(name, ask) : "offline";
    if (issue === null) {
      void send(name);
    } else {
      refuse(issue);
    }
  };
  const edited = (): void => {
    setRefusal(null);
  };
  return { pending, refusal, submit, edited };
}

interface DialogProps<Ask> {
  locale: Locale;
  ask: Ask;
  open: boolean;
  parts: DialogParts;
}

// The name dialog as drawn in one state: waiting for the answer or not,
// with the refusal that stands.
interface NameFormProps extends DialogProps<NameAsk> {
  pending: boolean;
  refusal: FolderRefusal | null;
  inputRef: RefObject<HTMLInputElement | null>;
  onSubmit: (event: SubmitEvent<HTMLFormElement>) => void;
  onEdit: () => void;
}

// One field for a folder's name: empty for a new folder, the name
// selected on a rename. The primary button waits for the server's answer
// and Escape holds meanwhile; the field keeps its text after a refusal.
export function NameForm(props: NameFormProps) {
  const { locale, ask, open, parts, pending, refusal, inputRef } = props;
  const { Dialog, DialogActions, Field, Button, close } = parts;
  const selectedRef = useRef(false);
  const selectOnce = (event: FocusEvent<HTMLInputElement>): void => {
    if (!selectedRef.current) {
      selectedRef.current = true;
      event.currentTarget.select();
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) {
          close();
        }
      }}
      onClosed={parts.settle}
      title={m.command_folder({ kind: ask.kind === "create" ? "new" : "rename" }, { locale })}
      description={
        ask.place === null
          ? m.folder_place_top({}, { locale })
          : m.folder_place_in({ parent: ask.place }, { locale })
      }
      failure={failureWords(refusal, locale)}
      initialFocus={inputRef}
    >
      <form className={styles.form} onSubmit={props.onSubmit}>
        <Field
          ref={inputRef}
          name={NAME_FIELD}
          label={m.folder_name_label({}, { locale })}
          type="text"
          autoComplete="off"
          defaultValue={ask.name}
          readOnly={pending}
          error={fieldWords(refusal, locale)}
          onChange={props.onEdit}
          onFocus={selectOnce}
        />
        <DialogActions>
          <Button type="button" held={pending} onClick={close}>
            {m.cancel_action({}, { locale })}
          </Button>
          <Button variant="primary" type="submit" pending={pending}>
            {ask.kind === "create"
              ? m.folder_create({}, { locale })
              : m.folder_rename({}, { locale })}
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  );
}

// The name dialog that sends the name and closes once the write landed.
export function NameDialog(props: DialogProps<NameAsk>) {
  const inputRef = useRef<HTMLInputElement>(null);
  const { pending, refusal, submit, edited } = useNameSubmit(
    props.ask,
    inputRef,
    props.parts.close,
  );
  return (
    <NameForm
      {...props}
      pending={pending}
      refusal={refusal}
      inputRef={inputRef}
      onSubmit={submit}
      onEdit={edited}
    />
  );
}

interface ConfirmWords {
  title: string;
  text: string;
  action: string;
}

function confirmWords({ kind, mailbox }: ConfirmAsk, locale: Locale): ConfirmWords {
  const count = mailbox.totalEmails;
  if (kind === "empty") {
    const role = mailbox.role ?? "";
    return {
      title: m.empty_title({ role }, { locale }),
      text: m.empty_text({ count }, { locale }),
      action: m.folder_empty({ role }, { locale }),
    };
  }
  return {
    title: m.folder_delete_title({ name: mailbox.name }, { locale }),
    text: m.folder_delete_text({ count }, { locale }),
    action: m.command_folder({ kind: "delete" }, { locale }),
  };
}

// The confirm before a destroy, emptying Trash or Junk or deleting a
// folder with its mail: Cancel holds the first focus and the action
// takes the filled danger style, so Enter does the safe thing. The
// action runs once the dialog has closed and handed the focus back, so
// what it takes away hands the focus on from where the dialog was opened.
export function ConfirmDialog({ locale, ask, open, parts }: DialogProps<ConfirmAsk>) {
  const { Dialog, DialogActions, Button, close } = parts;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmedRef = useRef(false);
  const words = confirmWords(ask, locale);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          close();
        }
      }}
      onClosed={() => {
        parts.settle();
        if (confirmedRef.current) {
          ask.confirm();
        }
      }}
      title={words.title}
      description={words.text}
      initialFocus={cancelRef}
    >
      <DialogActions>
        <Button ref={cancelRef} onClick={close}>
          {m.cancel_action({}, { locale })}
        </Button>
        <Button
          variant="dangerFilled"
          onClick={() => {
            confirmedRef.current = true;
            close();
          }}
        >
          {words.action}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

interface FolderDialogsProps {
  locale: Locale;
  asked: Asked;
  parts: DialogParts;
}

// The dialog the menus and the palette ask for, one at a time; a new ask
// starts its dialog afresh.
export function FolderDialogs({ locale, asked, parts }: FolderDialogsProps) {
  const { ask, open, seq } = asked;
  if (ask === null) {
    return null;
  }
  return "write" in ask ? (
    <NameDialog key={seq} locale={locale} ask={ask} open={open} parts={parts} />
  ) : (
    <ConfirmDialog key={seq} locale={locale} ask={ask} open={open} parts={parts} />
  );
}
