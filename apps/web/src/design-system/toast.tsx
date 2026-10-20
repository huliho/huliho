// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Toast } from "@base-ui/react/toast";
import { CircleAlert } from "lucide-react";
import type { ReactNode } from "react";

import { KeyCaps } from "../commands/key-caps";
import type { Chord } from "../commands/keys";
import { useCommand } from "../commands/use-command";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./toast.module.css";

// Undo over confirm: a destructive action waits this long before it is final.
export const UNDO_WINDOW_MS = 5_000;
const MS_PER_SECOND = 1_000;
// Toasts stack instead of hiding: a hidden toast is inert and its undo out of reach.
const TOAST_LIMIT = Number.POSITIVE_INFINITY;
const UNDO_KEYS: readonly Chord[] = [{ key: "z" }];

export interface ToastData {
  undo?: () => void;
  // Asks again for what failed; the toast's action calls it.
  retry?: () => void;
  // A change the server refused, which the alert circle marks.
  tone?: "danger";
}

export const toastManager = Toast.createToastManager<ToastData>();

export function ToastProvider({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toastManager} timeout={UNDO_WINDOW_MS} limit={TOAST_LIMIT}>
      {children}
    </Toast.Provider>
  );
}

// The key and the seconds after Undo; only the newest toast carries
// the key, so one key means one thing.
function UndoHint({ locale, keyed }: { locale: Locale; keyed: boolean }) {
  const seconds = new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "second",
    unitDisplay: "narrow",
  }).format(UNDO_WINDOW_MS / MS_PER_SECOND);
  return (
    <span className={styles.hint}>
      {keyed && (
        <>
          <KeyCaps keys={UNDO_KEYS} />
          <span aria-hidden="true">·</span>
        </>
      )}
      <span>{seconds}</span>
      <span className={styles.paused}>{m.toast_paused({}, { locale })}</span>
    </span>
  );
}

interface ToastItemProps {
  toast: Toast.Root.ToastObject<ToastData>;
  // The newest toast that can still undo, which answers the key.
  newest: boolean;
}

function ToastItem({ toast, newest }: ToastItemProps) {
  const locale = useLocale();
  const undo = toast.data?.undo;
  useCommand(
    undo !== undefined && newest
      ? {
          id: "undo",
          label: m.undo_action({}, { locale }),
          group: "act",
          keys: UNDO_KEYS,
          run: undo,
        }
      : null,
  );
  return (
    <Toast.Root toast={toast} className={styles.toast}>
      <Toast.Content className={styles.content}>
        {toast.data?.tone === "danger" && (
          <CircleAlert className={styles.alert} aria-hidden="true" />
        )}
        <Toast.Description className={styles.text} />
        {undo !== undefined && (
          <>
            <Toast.Action className={styles.undo}>{m.undo_action({}, { locale })}</Toast.Action>
            <UndoHint locale={locale} keyed={newest} />
          </>
        )}
        {toast.data?.retry !== undefined && (
          <Toast.Action className={styles.undo}>{m.retry_action({}, { locale })}</Toast.Action>
        )}
      </Toast.Content>
    </Toast.Root>
  );
}

// The newest toast first, as the manager holds them; the viewport draws
// it nearest its edge. A closing toast lets go of the key, so the next
// newest takes it.
export function Toasts() {
  const locale = useLocale();
  const { toasts } = Toast.useToastManager<ToastData>();
  const keyed = toasts.find(
    (toast) => toast.data?.undo !== undefined && toast.transitionStatus !== "ending",
  )?.id;
  return (
    <Toast.Portal>
      <Toast.Viewport className={styles.viewport} aria-label={m.toast_region({}, { locale })}>
        {toasts.map((toast) => (
          <ToastItem key={toast.id} toast={toast} newest={toast.id === keyed} />
        ))}
      </Toast.Viewport>
    </Toast.Portal>
  );
}
