// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { cx } from "./cx";
import styles from "./sheet.module.css";

const SPACE_STEPS = 6;
const RADII = ["--hhx-radius-control", "--hhx-radius-field", "--hhx-radius-overlay"];

function SpacingBars(): JSX.Element {
  const steps = Array.from({ length: SPACE_STEPS }, (_, index) => index + 1);
  return (
    <div className={styles.card}>
      {steps.map((step) => (
        <div key={step} className={styles.statusLine}>
          <span className={styles.bar} style={{ inlineSize: `var(--hhx-space-${String(step)})` }} />
          <span className={styles.swatchName}>--hhx-space-{step}</span>
        </div>
      ))}
    </div>
  );
}

function RadiusRow(): JSX.Element {
  return (
    <div className={styles.shapeRow}>
      {RADII.map((radius) => (
        <div key={radius} className={styles.shapeSample} style={{ borderRadius: `var(${radius})` }}>
          {radius}
        </div>
      ))}
    </div>
  );
}

function ElevationRow(): JSX.Element {
  return (
    <div className={styles.shapeRow}>
      <div className={styles.shapeSample}>border only</div>
      <div className={cx(styles.shapeSample, styles.halo)}>overlay halo</div>
    </div>
  );
}

const meta: Meta = {
  title: "Tokens/Spacing and shape",
};

export default meta;

export const Sheet: StoryObj = {
  render: () => (
    <div className={styles.sheet}>
      <h2 className={styles.sectionTitle}>Spacing, 4 px grid</h2>
      <SpacingBars />
      <h2 className={styles.sectionTitle}>Radius</h2>
      <RadiusRow />
      <h2 className={styles.sectionTitle}>Elevation by border</h2>
      <ElevationRow />
    </div>
  ),
};
