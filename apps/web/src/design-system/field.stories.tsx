// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { Field } from "./field";
import styles from "./sheet.module.css";

const meta: Meta = {
  title: "Primitives/Field",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <div className={styles.card}>
      <Field label="Name" type="text" autoComplete="username" defaultValue="mira@example.com" />
    </div>
  ),
};

export const WithError: StoryObj = {
  render: () => (
    <div className={styles.card}>
      <Field
        label="Password"
        type="password"
        autoComplete="current-password"
        defaultValue="example passphrase"
        error="Couldn’t sign in. Check the name and password, then try again."
      />
    </div>
  ),
};
