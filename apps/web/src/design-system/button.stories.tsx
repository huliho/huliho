// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { Button } from "./button";
import styles from "./sheet.module.css";

const meta: Meta = {
  title: "Primitives/Button",
};

export default meta;

export const Variants: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <Button variant="primary">Sign in</Button>
      <Button>Try again</Button>
      <Button variant="danger">Revoke all others</Button>
    </div>
  ),
};

export const States: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <Button variant="primary" pending>
        Signing in…
      </Button>
      <Button variant="primary" held>
        Try again in 01:32
      </Button>
    </div>
  ),
};
