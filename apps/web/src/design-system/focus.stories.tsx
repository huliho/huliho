// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { cx } from "./cx";
import styles from "./sheet.module.css";

function FocusSamples(): JSX.Element {
  return (
    <div className={styles.card}>
      <button type="button" className={cx(styles.button, styles.focusRingSample)}>
        Send
      </button>
      <input
        type="text"
        className={cx(styles.input, styles.focusRingSample)}
        defaultValue="jmap.fastmail.example"
        aria-label="Server"
      />
      <div className={styles.rowList}>
        <div className={cx(styles.row, styles.rowSelected, styles.focusRingInsetSample)}>
          <span>Selected and focused: the inset ring renders over the tint</span>
        </div>
      </div>
      <span className={styles.muted}>
        2 px accent ring with a 2 px offset, inset inside lists. When custom colors are stripped the
        ring falls back to the text color.
      </span>
    </div>
  );
}

const meta: Meta = {
  title: "Tokens/Focus",
};

export default meta;

export const Ring: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Focus ring</h2>
      <FocusSamples />
    </div>
  ),
};
