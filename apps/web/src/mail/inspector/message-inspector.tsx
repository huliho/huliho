// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Authentication } from "@huliho/core";
import { X } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";

import { ESCAPE } from "../../commands/keys";
import type { Chord } from "../../commands/keys";
import { useCommand } from "../../commands/use-command";
import { focusHeir } from "../../design-system/button";
import buttonStyles from "../../design-system/button.module.css";
import { cx } from "../../design-system/cx";
import { Dialog, DialogActions } from "../../design-system/dialog";
import iconButton from "../../design-system/icon-button.module.css";
import { Tabs } from "../../design-system/tabs";
import type { TabItem } from "../../design-system/tabs";
import { TextLines } from "../../design-system/text-lines";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { shortSentence } from "../message-body";
import type { Short } from "../message-body";
import { AuthenticationLine } from "./authentication-line";
import { SourcePanel } from "./source-panel";
import type { SourceAsk } from "./use-source";
import styles from "./message-inspector.module.css";

export type InspectorTab = "rendered" | "plain" | "source";

// Escape inside the message's frame reaches the registry and not the
// dialog; this command closes the inspector from there, ahead of the
// thread.
const CLOSE_KEYS: readonly Chord[] = [ESCAPE];

// The plain text as the card draws it and why it stops short, if it does.
export interface InspectedPlain {
  text: ReactNode;
  short: Short | null;
}

// What the inspector shows of a message once its body is in.
export interface InspectedBody {
  // What the topmost Authentication-Results header says.
  authentication: Authentication;
  // Null for a message without a text part.
  plain: InspectedPlain | null;
  // Where the whole message downloads from.
  download: string;
  source: SourceAsk;
}

export interface MessageInspectorProps {
  locale: Locale;
  open: boolean;
  onClose: () => void;
  // Runs once the closing fade has ended.
  onClosed: () => void;
  // The element that takes the focus back; undefined leaves that to the dialog.
  opener: HTMLElement | undefined;
  // The body as the card shows it, in whatever state it is in.
  rendered: ReactNode;
  // Null while the body is on its way.
  body: InspectedBody | null;
  initialTab?: InspectorTab | undefined;
}

function tabsOf(locale: Locale): TabItem<InspectorTab>[] {
  return [
    { value: "rendered", label: m.inspector_tab_rendered({}, { locale }) },
    { value: "plain", label: m.inspector_tab_plain({}, { locale }) },
    { value: "source", label: m.inspector_tab_source({}, { locale }) },
  ];
}

// The plain text of the message with the sentence of why it stops
// short, the sentence for one without a text part and the still lines
// while the body is on its way. The way to the rest is the download in
// the foot.
function PlainPanel({ locale, body }: { locale: Locale; body: InspectedBody | null }) {
  if (body === null) {
    return <TextLines label={m.loading_label({}, { locale })} />;
  }
  if (body.plain === null) {
    return <p className={styles.sentence}>{m.inspector_no_plain({}, { locale })}</p>;
  }
  return (
    <>
      {body.plain.text}
      {body.plain.short !== null && (
        <p className={styles.cut}>{shortSentence(body.plain.short, locale)}</p>
      )}
    </>
  );
}

function CloseButton({ locale, onClose }: { locale: Locale; onClose: () => void }) {
  return (
    <button
      type="button"
      className={iconButton.button}
      aria-label={m.inspector_close({}, { locale })}
      onClick={onClose}
    >
      <X className={iconButton.icon} aria-hidden="true" />
    </button>
  );
}

// The three views, each in a box that takes the focus of a control
// that leaves.
function panelsOf(
  locale: Locale,
  rendered: ReactNode,
  body: InspectedBody | null,
): Record<InspectorTab, ReactNode> {
  return {
    rendered: <div {...focusHeir}>{rendered}</div>,
    plain: (
      <div {...focusHeir}>
        <PlainPanel locale={locale} body={body} />
      </div>
    ),
    source: (
      <div {...focusHeir}>
        <SourcePanel locale={locale} ask={body?.source ?? null} />
      </div>
    ),
  };
}

// The message in three views: as the card renders it, as plain text and
// as the raw source the server holds, with the receiving server's
// verdict above the tabs and the download in the foot. Full screen on a
// phone, the wide dialog from the tablet width on; the first focus
// lands on the close button, as in every dialog with a header.
export function MessageInspector(props: MessageInspectorProps) {
  const { locale, open, onClose, onClosed, opener, rendered, body, initialTab } = props;
  const [tab, setTab] = useState<InspectorTab>(initialTab ?? "rendered");
  useCommand(
    open
      ? {
          id: "inspector.close",
          label: m.inspector_close({}, { locale }),
          group: "navigate",
          keys: CLOSE_KEYS,
          run: onClose,
        }
      : null,
  );
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
      onClosed={onClosed}
      title={m.inspector_title({}, { locale })}
      size="screen"
      finalFocus={opener}
      header={<CloseButton locale={locale} onClose={onClose} />}
    >
      <div className={styles.views}>
        {body !== null && (
          <AuthenticationLine locale={locale} authentication={body.authentication} />
        )}
        <Tabs
          value={tab}
          onValueChange={setTab}
          tabs={tabsOf(locale)}
          panelClassName={styles.sheet}
          panels={panelsOf(locale, rendered, body)}
        />
      </div>
      {body !== null && (
        <DialogActions>
          <a
            className={cx(buttonStyles.button, buttonStyles.secondary, styles.download)}
            href={body.download}
            download
          >
            {m.body_download({}, { locale })}
          </a>
        </DialogActions>
      )}
    </Dialog>
  );
}
