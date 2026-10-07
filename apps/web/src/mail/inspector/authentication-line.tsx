// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Authentication, AuthVerdict } from "@huliho/core";
import { CircleCheck, CircleHelp, CircleMinus, CircleX } from "lucide-react";
import { Fragment } from "react";
import type { ReactNode } from "react";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import styles from "./message-inspector.module.css";

// Stands in for each value inside the sentence, so the words around it
// fall where the translation puts them: private-use code points, which
// no sentence holds.
const PRIVATE_USE = 0xe0_00;
const SLOT = {
  server: String.fromCodePoint(PRIVATE_USE),
  spf: String.fromCodePoint(PRIVATE_USE + 1),
  dkim: String.fromCodePoint(PRIVATE_USE + 2),
  dmarc: String.fromCodePoint(PRIVATE_USE + 3),
} as const;
// Cuts the sentence at every private-use code point and keeps each one.
const SLOTS = /(\p{Co})/u;

// The mark beside a result, unheard: the word says it.
function markOf(verdict: AuthVerdict): ReactNode {
  switch (verdict) {
    case "pass":
      return <CircleCheck className={styles.mark} aria-hidden="true" />;
    case "fail":
      return <CircleX className={styles.mark} aria-hidden="true" />;
    case "none":
      return <CircleMinus className={styles.mark} aria-hidden="true" />;
    default:
      return <CircleHelp className={styles.mark} aria-hidden="true" />;
  }
}

function wordOf(verdict: AuthVerdict, locale: Locale): string {
  switch (verdict) {
    case "pass":
      return m.inspector_result_pass({}, { locale });
    case "fail":
      return m.inspector_result_fail({}, { locale });
    case "none":
      return m.inspector_result_none({}, { locale });
    default:
      return m.inspector_result_unknown({}, { locale });
  }
}

// One method's result: a mark and a word, read as the word alone.
function Result({ verdict, locale }: { verdict: AuthVerdict; locale: Locale }) {
  return (
    <span className={styles.result} data-verdict={verdict}>
      {markOf(verdict)}
      {wordOf(verdict, locale)}
    </span>
  );
}

// The sentence with its values put back in as elements, each piece
// keyed by where it starts. The empty piece between two slots that
// touch is left out, so no two pieces share a key.
function filled(sentence: string, values: ReadonlyMap<string, ReactNode>): ReactNode[] {
  const pieces: ReactNode[] = [];
  let start = 0;
  for (const piece of sentence.split(SLOTS)) {
    if (piece !== "") {
      pieces.push(<Fragment key={start}>{values.get(piece) ?? piece}</Fragment>);
    }
    start += piece.length;
  }
  return pieces;
}

interface AuthenticationLineProps {
  locale: Locale;
  authentication: Authentication;
}

// What the receiving server found, as one sentence: the server by its
// name in mono (your mail server, for a header that names none) and
// each method's result. A message without the header or with one that
// cannot be read shows nothing.
export function AuthenticationLine({ locale, authentication }: AuthenticationLineProps) {
  if (authentication.status !== "parsed") {
    return null;
  }
  const { server, spf, dkim, dmarc } = authentication.results;
  const values = new Map<string, ReactNode>([
    [
      SLOT.server,
      <bdi key="server" className={styles.host}>
        {server}
      </bdi>,
    ],
    [SLOT.spf, <Result key="spf" verdict={spf} locale={locale} />],
    [SLOT.dkim, <Result key="dkim" verdict={dkim} locale={locale} />],
    [SLOT.dmarc, <Result key="dmarc" verdict={dmarc} locale={locale} />],
  ]);
  const results = { spf: SLOT.spf, dkim: SLOT.dkim, dmarc: SLOT.dmarc };
  const sentence =
    server === null || server === ""
      ? m.inspector_checked_own(results, { locale })
      : m.inspector_checked({ server: SLOT.server, ...results }, { locale });
  return <p className={styles.line}>{filled(sentence, values)}</p>;
}
