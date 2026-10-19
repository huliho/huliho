// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { Checkbox } from "./checkbox";
import styles from "./sheet.module.css";

const meta: Meta = {
  title: "Primitives/Checkbox",
};

export default meta;

function ignore(): void {
  // A drawn state keeps its tick.
}

// Off and on, side by side.
export const States: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <Checkbox checked={false} label="Off" onCheckedChange={ignore} />
      <Checkbox checked label="On" onCheckedChange={ignore} />
    </div>
  ),
};
