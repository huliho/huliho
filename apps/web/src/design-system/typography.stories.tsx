// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import styles from "./sheet.module.css";

const SCALE = [
  { token: "--hhx-type-1", use: "meta, keys", sample: "Yesterday 14:32 · 2.4 MB" },
  { token: "--hhx-type-2", use: "body, rows", sample: "Jonas Verhoeven: notes from the standup" },
  { token: "--hhx-type-3", use: "emphasis, inputs", sample: "About your draft budget" },
  { token: "--hhx-type-4", use: "pane titles", sample: "Inbox" },
  { token: "--hhx-type-5", use: "screen titles", sample: "Add an account" },
];

const WEIGHTS = [400, 500, 600, 700];

function ScaleList(): JSX.Element {
  return (
    <div className={styles.card}>
      {SCALE.map((step) => (
        <p key={step.token} style={{ margin: 0, fontSize: `var(${step.token})` }}>
          {step.sample}{" "}
          <span className={styles.swatchName}>
            {step.token} · {step.use}
          </span>
        </p>
      ))}
    </div>
  );
}

function WeightRow(): JSX.Element {
  return (
    <div className={styles.card}>
      {WEIGHTS.map((weight) => (
        <p key={weight} style={{ margin: 0, fontWeight: weight }}>
          Hanken Grotesk {String(weight)}: Zoë sent her résumé from the café in Curaçao
        </p>
      ))}
    </div>
  );
}

function MonoSamples(): JSX.Element {
  return (
    <div className={styles.card}>
      <span className={styles.mono}>14:32 · 23 unread · jmap.fastmail.example</span>
      <span>
        Jump to a mailbox with <kbd className={styles.kbd}>g</kbd> then{" "}
        <kbd className={styles.kbd}>i</kbd>, open search with <kbd className={styles.kbd}>/</kbd>
      </span>
    </div>
  );
}

const meta: Meta = {
  title: "Tokens/Typography",
};

export default meta;

export const Scale: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Type scale</h2>
      <ScaleList />
      <h2 className={styles.sectionTitle}>Weights</h2>
      <WeightRow />
      <h2 className={styles.sectionTitle}>Mono</h2>
      <MonoSamples />
    </div>
  ),
};
