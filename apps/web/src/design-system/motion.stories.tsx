// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";
import { useState } from "react";

import { cx } from "./cx";
import styles from "./sheet.module.css";

const DURATIONS = [
  { token: "--hhx-duration-flip", use: "state flips" },
  { token: "--hhx-duration-reveal", use: "pane reveal, the signature" },
  { token: "--hhx-duration-overlay", use: "overlays" },
];

function SignatureDemo(): JSX.Element {
  const [open, setOpen] = useState(false);
  return (
    <div className={styles.card}>
      <button
        type="button"
        className={styles.button}
        onClick={() => {
          setOpen((current) => !current);
        }}
      >
        {open ? "Close the message" : "Open the message"}
      </button>
      <article
        className={cx(styles.card, styles.panel, open ? undefined : styles.panelHidden)}
        aria-hidden={!open}
      >
        <strong>Marit Deelstra</strong>
        <span className={styles.muted}>
          The opened message fades in while sliding in from the list.
        </span>
      </article>
    </div>
  );
}

function DurationList(): JSX.Element {
  return (
    <div className={styles.card}>
      {DURATIONS.map((duration) => (
        <span key={duration.token} className={styles.statusLine}>
          <span className={styles.mono}>{duration.token}</span>
          <span className={styles.muted}>{duration.use}</span>
        </span>
      ))}
      <span className={styles.muted}>
        One ease-out curve, only on user-initiated change. Reduced motion turns the slide into a
        plain fade.
      </span>
    </div>
  );
}

const meta: Meta = {
  title: "Tokens/Motion",
};

export default meta;

export const Signature: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Signature: pane reveal</h2>
      <SignatureDemo />
      <h2 className={styles.sectionTitle}>Durations</h2>
      <DurationList />
    </div>
  ),
};
