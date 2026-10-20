// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { useRef, useSyncExternalStore } from "react";

import { awaitLayer } from "../../commands/registry";
import { Button } from "../../design-system/button";
import { Dialog, DialogActions } from "../../design-system/dialog";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// What the dialog asks about: the Trash or Junk to empty and what runs
// once the user confirms.
export interface EmptyRequest {
  mailbox: Mailbox;
  confirm: () => void;
}

// The request stays through the closing fade, so the words stay put.
interface Asked {
  request: EmptyRequest | null;
  open: boolean;
}

let asked: Asked = { request: null, open: false };
const listeners = new Set<() => void>();

function publish(next: Asked): void {
  asked = next;
  for (const listener of listeners) {
    listener();
  }
}

export function askEmpty(request: EmptyRequest): void {
  awaitLayer();
  publish({ request, open: true });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function read(): Asked {
  return asked;
}

function close(): void {
  publish({ ...asked, open: false });
}

interface EmptyDialogProps {
  locale: Locale;
  // The Trash or Junk to empty, whose role and count the words take.
  mailbox: Mailbox;
  open: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  // Runs once the closing fade has ended.
  onClosed?: () => void;
}

// The confirm before a mailbox is emptied, the product's one for a bulk
// destroy: Cancel holds the first focus and the action takes the filled
// danger style, so Enter does the safe thing.
export function EmptyDialog(props: EmptyDialogProps) {
  const { locale, mailbox, open, onCancel, onConfirm } = props;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const role = mailbox.role ?? "";
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onCancel();
        }
      }}
      onClosed={props.onClosed}
      title={m.empty_title({ role }, { locale })}
      description={m.empty_text({ count: mailbox.totalEmails }, { locale })}
      initialFocus={cancelRef}
    >
      <DialogActions>
        <Button ref={cancelRef} onClick={onCancel}>
          {m.cancel_action({}, { locale })}
        </Button>
        <Button variant="dangerFilled" onClick={onConfirm}>
          {m.folder_empty({ role }, { locale })}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// The dialog as the menus and the palette ask for it.
export function ConfirmEmpty({ locale }: { locale: Locale }) {
  const { request, open } = useSyncExternalStore(subscribe, read, read);
  if (request === null) {
    return null;
  }
  return (
    <EmptyDialog
      locale={locale}
      mailbox={request.mailbox}
      open={open}
      onCancel={close}
      onConfirm={() => {
        request.confirm();
        close();
      }}
      onClosed={() => {
        publish({ request: null, open: false });
      }}
    />
  );
}
