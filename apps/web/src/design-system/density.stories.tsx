// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { cx } from "./cx";
import styles from "./sheet.module.css";

const ROWS = [
  { sender: "Marit Deelstra", subject: "Planning the next sprint", time: "14:32", unread: true },
  { sender: "Jonas Verhoeven", subject: "Re: draft budget", time: "11:05", unread: false },
  { sender: "Fem Roskam", subject: "Server maintenance window", time: "Mon", unread: false },
];

const VALUE_TOKENS = [
  "--hhx-row-height",
  "--hhx-type-row",
  "--hhx-leading",
  "--hhx-hit-target",
  "--hhx-pane-padding",
  "--hhx-toolbar-height",
];

function SampleRows(): JSX.Element {
  return (
    <div className={styles.rowList}>
      {ROWS.map((row, index) => (
        <div
          key={row.sender}
          className={cx(
            styles.row,
            row.unread ? styles.rowUnread : undefined,
            index === 1 ? styles.rowSelected : undefined,
          )}
        >
          {row.unread ? <span className={styles.unreadDot} /> : null}
          <span>{row.sender}</span>
          <span className={styles.muted}>{row.subject}</span>
          <span className={styles.rowTime}>{row.time}</span>
        </div>
      ))}
    </div>
  );
}

function DensitySheet({ density }: { density: string }): JSX.Element {
  return (
    <div className={styles.sheet} data-density={density}>
      <h2 className={styles.sectionTitle}>Density: {density}</h2>
      <SampleRows />
      <div className={styles.card}>
        {VALUE_TOKENS.map((token) => (
          <span key={token} className={styles.swatchName}>
            {token}
          </span>
        ))}
      </div>
    </div>
  );
}

const meta: Meta = {
  title: "Tokens/Density",
};

export default meta;

export const Comfortable: StoryObj = {
  render: () => <DensitySheet density="comfortable" />,
};

export const Compact: StoryObj = {
  render: () => <DensitySheet density="compact" />,
};

export const Touch: StoryObj = {
  render: () => <DensitySheet density="touch" />,
};
