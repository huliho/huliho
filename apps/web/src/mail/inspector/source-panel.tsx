// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { ErrorState } from "../../design-system/error-state";
import { TextLines } from "../../design-system/text-lines";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { OfflineNotice } from "../message-body";
import { SOURCE_VIEW_BYTES, revealControls } from "./source";
import { useSource } from "./use-source";
import type { SourceAsk } from "./use-source";
import styles from "./message-inspector.module.css";

const KIB = 1024;

// The cap as the cut notice names it, in the unit the body's cut notice uses.
function capSize(locale: Locale): string {
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "kilobyte",
    maximumFractionDigits: 0,
  }).format(SOURCE_VIEW_BYTES / KIB);
}

interface SourceViewProps {
  locale: Locale;
  ask: SourceAsk;
}

// The raw message in a mono block that never wraps, with the cut notice
// under it where the message runs past the cap. The panel around it
// scrolls sideways and takes the focus, so the keyboard scrolls it too.
// Offline it says what the body says: the source is never on the device.
function SourceView({ locale, ask }: SourceViewProps) {
  const state = useSource(ask);
  switch (state.kind) {
    case "loading":
      return <TextLines label={m.loading_label({}, { locale })} />;
    case "offline":
      return <OfflineNotice locale={locale} />;
    case "error":
      return (
        <ErrorState
          variant="inline"
          message={m.body_error({}, { locale })}
          retryLabel={m.retry_action({}, { locale })}
          onRetry={state.retry}
        />
      );
    default:
      return (
        <>
          <pre dir="ltr" className={styles.source}>
            {revealControls(state.source.text)}
          </pre>
          {state.source.more && (
            <p className={styles.cut}>{m.inspector_cut({ size: capSize(locale) }, { locale })}</p>
          )}
        </>
      );
  }
}

interface SourcePanelProps {
  locale: Locale;
  ask: SourceAsk | null;
}

// The source once the body says where it is; the still lines until then.
export function SourcePanel({ locale, ask }: SourcePanelProps) {
  return ask === null ? (
    <TextLines label={m.loading_label({}, { locale })} />
  ) : (
    <SourceView locale={locale} ask={ask} />
  );
}
