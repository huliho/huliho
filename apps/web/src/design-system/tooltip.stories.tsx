// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { Archive, Star } from "lucide-react";

import iconButton from "./icon-button.module.css";
import styles from "./sheet.module.css";
import { Tooltip } from "./tooltip";

const meta: Meta = {
  title: "Primitives/Tooltip",
};

export default meta;

// A hint open under its control: the label, then the key as a cap draws it.
export const WithKey: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <Tooltip label="Archive" keys={[{ key: "e" }]} defaultOpen>
        <button type="button" className={iconButton.button} aria-label="Archive">
          <Archive className={iconButton.icon} aria-hidden="true" />
        </button>
      </Tooltip>
    </div>
  ),
};

// A control without a key gets its label alone.
export const LabelAlone: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <Tooltip label="Remove star" keys={[]} defaultOpen>
        <button type="button" className={iconButton.button} aria-label="Remove star">
          <Star className={iconButton.icon} aria-hidden="true" />
        </button>
      </Tooltip>
    </div>
  ),
};
