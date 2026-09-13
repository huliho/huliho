// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import { cx } from "../../design-system/cx";
import styles from "./add-account.module.css";

type Tone = "info" | "warn" | "danger";

interface CalloutProps {
  tone: Tone;
  // Alert for a refusal, status for a wait; nothing for a plain sentence.
  live?: "alert" | "status" | undefined;
  children: ReactNode;
}

function dotClass(tone: Tone): string | undefined {
  if (tone === "warn") {
    return styles.dotWarn;
  }
  return tone === "danger" ? styles.dotDanger : undefined;
}

// The sentence box: a dot in the tone's color next to the text. A status
// is an output element, which carries that role on its own.
export function Callout({ tone, live, children }: CalloutProps) {
  const Box = live === "status" ? "output" : "div";
  return (
    <Box
      className={cx(styles.callout, tone === "danger" ? styles.calloutDanger : undefined)}
      role={live === "alert" ? live : undefined}
    >
      <span className={cx(styles.dot, dotClass(tone))} aria-hidden="true" />
      <span className={styles.calloutText}>{children}</span>
    </Box>
  );
}
