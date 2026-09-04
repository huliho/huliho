// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Toast } from "@base-ui/react/toast";
import type { ReactNode } from "react";

import { useCommand } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import type { Locale } from "../paraglide/runtime.js";
import { Kbd } from "./kbd";
import styles from "./toast.module.css";

// Undo over confirm: a destructive action waits this long before it is final.
export const UNDO_WINDOW_MS = 5_000;
const MS_PER_SECOND = 1_000;
// Toasts stack instead of hiding: a hidden toast is inert and its undo out of reach.
const TOAST_LIMIT = Number.POSITIVE_INFINITY;
const UNDO_KEY = "z";

export interface ToastData {
  undo?: () => void;
}

export const toastManager = Toast.createToastManager<ToastData>();

export function ToastProvider({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toastManager} timeout={UNDO_WINDOW_MS} limit={TOAST_LIMIT}>
      {children}
    </Toast.Provider>
  );
}

function UndoHint({ locale }: { locale: Locale }) {
  const seconds = new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "second",
    unitDisplay: "narrow",
  }).format(UNDO_WINDOW_MS / MS_PER_SECOND);
  return (
    <span className={styles.hint}>
      <Kbd>{UNDO_KEY}</Kbd>
      <span aria-hidden="true">·</span>
      <span>{seconds}</span>
      <span className={styles.paused}>{m.toast_paused({}, { locale })}</span>
    </span>
  );
}

function ToastItem({ toast }: { toast: Toast.Root.ToastObject<ToastData> }) {
  const locale = getLocale();
  const undo = toast.data?.undo;
  // A closing toast lets go of the key, so the next undo can take it.
  const claimsKey = undo !== undefined && toast.transitionStatus !== "ending";
  useCommand(claimsKey ? { id: "undo", key: UNDO_KEY, run: undo } : null);
  return (
    <Toast.Root toast={toast} className={styles.toast}>
      <Toast.Content className={styles.content}>
        <Toast.Description className={styles.text} />
        {undo !== undefined && (
          <>
            <Toast.Action className={styles.undo}>{m.undo_action({}, { locale })}</Toast.Action>
            <UndoHint locale={locale} />
          </>
        )}
      </Toast.Content>
    </Toast.Root>
  );
}

export function Toasts() {
  const locale = getLocale();
  const { toasts } = Toast.useToastManager<ToastData>();
  return (
    <Toast.Portal>
      <Toast.Viewport className={styles.viewport} aria-label={m.toast_region({}, { locale })}>
        {toasts.map((toast) => (
          <ToastItem key={toast.id} toast={toast} />
        ))}
      </Toast.Viewport>
    </Toast.Portal>
  );
}
