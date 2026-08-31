// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { cx } from "./cx";
import styles from "./sheet.module.css";

const ROLES = [
  "bg",
  "surface",
  "hover",
  "border",
  "border-strong",
  "border-control",
  "text",
  "text-muted",
  "accent",
  "accent-strong",
  "accent-tint",
  "danger",
  "warn",
  "success",
];

function RoleGrid(): JSX.Element {
  return (
    <div className={styles.grid}>
      {ROLES.map((role) => (
        <figure key={role} className={styles.swatch} style={{ margin: 0 }}>
          <div className={styles.swatchChip} style={{ background: `var(--hh-${role})` }} />
          <figcaption className={styles.swatchName}>--hh-{role}</figcaption>
        </figure>
      ))}
    </div>
  );
}

function SampleCard(): JSX.Element {
  return (
    <article className={styles.card}>
      <strong>Marit Deelstra</strong>
      <span>Planning the next sprint: who takes the migrations?</span>
      <span className={styles.muted}>
        I attached the notes from Thursday. Could you respond before Wednesday?
      </span>
      <a className={styles.accentText} href="#renamed-thread">
        Show the whole conversation
      </a>
      <span className={cx(styles.statusLine, styles.dangerText)}>
        <span className={styles.statusDot} /> Couldn&apos;t send. Kept as draft.
      </span>
      <span className={cx(styles.statusLine, styles.warnText)}>
        <span className={styles.statusDot} /> Storage almost full: 9.1 of 10 GB used.
      </span>
      <span className={cx(styles.statusLine, styles.successText)}>
        <span className={styles.statusDot} /> Account connected. Loading the first mail.
      </span>
    </article>
  );
}

const meta: Meta = {
  title: "Tokens/Colors",
};

export default meta;

export const Roles: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Semantic roles</h2>
      <RoleGrid />
    </div>
  ),
};

export const InUse: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Roles in use</h2>
      <SampleCard />
    </div>
  ),
};
