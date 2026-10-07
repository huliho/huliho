// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { WifiOff } from "lucide-react";
import { useRef } from "react";

import { LeavingButton } from "../design-system/button";
import buttonStyles from "../design-system/button.module.css";
import { cx } from "../design-system/cx";
import { ErrorState } from "../design-system/error-state";
import { TextLines } from "../design-system/text-lines";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { FRAME_SANDBOX } from "./body/frame-document";
import { useFrame } from "./body/use-frame";
import { PlainText } from "./plain-text";
import type { LinkPolicy } from "./plain-text";
import { useStatusText } from "./status-text";
import styles from "./message-body.module.css";

// Why a body stops short: cut by the server at the cap it was asked at,
// or longer than the card draws.
export type Short = { kind: "size"; size: string } | { kind: "lines" };

// The sentence that says why a body stops short.
export function shortSentence(short: Short, locale: Locale): string {
  return short.kind === "size"
    ? m.body_cut({ size: short.size }, { locale })
    : m.body_too_long({}, { locale });
}

// What a body that stops short offers. Cut at the cap it was asked at:
// the size, the second ask until it was made and the download once that
// ask came back cut as well. Longer than the card draws: the download.
export type Cut =
  | {
      kind: "size";
      size: string;
      whole: (() => void) | null;
      pending: boolean;
      download: string | null;
    }
  | { kind: "lines"; download: string };

// What the text slot of an open card shows.
export type BodyView =
  | { kind: "loading" }
  | { kind: "error"; retry: () => void }
  | { kind: "offline" }
  | { kind: "gone" }
  | { kind: "none" }
  | { kind: "complex" }
  | {
      kind: "text";
      text: string;
      flowed: { delSp: boolean } | null;
      links: LinkPolicy;
      cut: Cut | null;
    }
  | {
      kind: "html";
      // The frame's document.
      html: string;
      title: string;
      // A light-only message shown as sent in the dark theme stands on
      // its own white page, inset in the card.
      asSent: boolean;
      cut: Cut | null;
    };

function Lines({ locale }: { locale: Locale }) {
  return <TextLines label={m.loading_label({}, { locale })} />;
}

// The sentence of a message this device never got. Its status region
// stands empty first, so the sentence is announced when it lands.
export function OfflineNotice({ locale }: { locale: Locale }) {
  const sentenceRef = useRef<HTMLSpanElement>(null);
  useStatusText(sentenceRef, m.body_offline({}, { locale }));
  return (
    <p className={styles.notice}>
      <WifiOff className={styles.icon} aria-hidden="true" />
      <span ref={sentenceRef} role="status" />
    </p>
  );
}

// The notice under a body that stops short: why, and the way to the rest,
// behind a hairline so it reads as the card's, not the mail's. The
// button for the rest leaves with the answer.
function CutNotice({ locale, cut }: { locale: Locale; cut: Cut }) {
  return (
    <div className={styles.cut}>
      <p className={styles.sentence}>{shortSentence(cut, locale)}</p>
      {cut.kind === "size" && cut.whole !== null && (
        <LeavingButton pending={cut.pending} onClick={cut.whole}>
          {m.body_show_whole({}, { locale })}
        </LeavingButton>
      )}
      {cut.download !== null && (
        <a className={cx(buttonStyles.button, buttonStyles.secondary)} href={cut.download} download>
          {m.body_download({}, { locale })}
        </a>
      )}
    </div>
  );
}

interface HtmlFrameProps {
  locale: Locale;
  html: string;
  title: string;
  asSent: boolean;
}

// The sandboxed frame a message renders in, as tall as its content; the
// still lines stand in front of it until its document loaded.
function HtmlFrame({ locale, html, title, asSent }: HtmlFrameProps) {
  const { ref, height } = useFrame(html);
  return (
    <>
      {height === null && <Lines locale={locale} />}
      <iframe
        ref={ref}
        title={title}
        sandbox={FRAME_SANDBOX}
        className={styles.frame}
        data-canvas={asSent ? "as-sent" : undefined}
        data-loading={height === null || undefined}
        style={{ blockSize: height ?? 0 }}
      />
    </>
  );
}

function sentenceOf(kind: "gone" | "none" | "complex", locale: Locale): string {
  switch (kind) {
    case "gone":
      return m.body_gone({}, { locale });
    case "none":
      return m.body_no_text({}, { locale });
    default:
      return m.body_too_complex({}, { locale });
  }
}

// The text slot of an open card: the body as the frame or as text, or
// the one sentence that says why there is none yet.
export function MessageBody({ locale, view }: { locale: Locale; view: BodyView }) {
  switch (view.kind) {
    case "loading":
      return <Lines locale={locale} />;
    case "error":
      return (
        <ErrorState
          variant="inline"
          message={m.body_error({}, { locale })}
          retryLabel={m.retry_action({}, { locale })}
          onRetry={view.retry}
        />
      );
    case "offline":
      return <OfflineNotice locale={locale} />;
    case "text":
      return (
        <>
          <PlainText text={view.text} flowed={view.flowed} links={view.links} />
          {view.cut !== null && <CutNotice locale={locale} cut={view.cut} />}
        </>
      );
    case "html":
      return (
        <>
          <HtmlFrame locale={locale} html={view.html} title={view.title} asSent={view.asSent} />
          {view.cut !== null && <CutNotice locale={locale} cut={view.cut} />}
        </>
      );
    default:
      return <p className={styles.sentence}>{sentenceOf(view.kind, locale)}</p>;
  }
}
