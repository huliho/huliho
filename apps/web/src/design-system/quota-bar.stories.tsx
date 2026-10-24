// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { QuotaBar } from "./quota-bar";
import styles from "./sheet.module.css";
import { Tooltip } from "./tooltip";

const meta: Meta = {
  title: "Primitives/QuotaBar",
};

export default meta;

const GB = 1000 ** 3;
const LIMIT = 10 * GB;
const PLAIN = {
  used: 4.2 * GB,
  limit: LIMIT,
  locale: "en",
  words: "4.2 of 10 GB",
  detail: "Mail storage: 4.2 GB of 10 GB used",
  warning: null,
};
const NEARLY_FULL = {
  used: 9.1 * GB,
  limit: LIMIT,
  locale: "en",
  words: "9.1 of 10 GB",
  detail: "Mail storage: 9.1 GB of 10 GB used",
  warning: "Almost full",
};

// The sidebar's foot: the figures over an accent bar.
export const Foot: StoryObj = {
  render: () => (
    <div className={styles.sidebarFoot}>
      <QuotaBar {...PLAIN} variant="foot" />
    </div>
  ),
};

// Past the warn limit: the word before the figures and the bar in warn.
export const AlmostFull: StoryObj = {
  render: () => (
    <div className={styles.sidebarFoot}>
      <QuotaBar {...NEARLY_FULL} variant="foot" />
    </div>
  ),
};

// A server that counts messages alone.
export const Messages: StoryObj = {
  render: () => (
    <div className={styles.sidebarFoot}>
      <QuotaBar
        used={1204}
        limit={10_000}
        locale="en"
        words="1,204 of 10,000 messages"
        detail="Mail storage: 1,204 of 10,000 messages"
        warning={null}
        variant="foot"
      />
    </div>
  ),
};

// The rail's bar alone, plain and past the warn limit.
export const Rail: StoryObj = {
  render: () => (
    <div className={styles.shapeRow}>
      <QuotaBar {...PLAIN} variant="rail" />
      <QuotaBar {...NEARLY_FULL} variant="rail" />
    </div>
  ),
};

// The hint over the foot names the exact figures.
export const Hint: StoryObj = {
  render: () => (
    <div className={styles.sidebarFoot}>
      <Tooltip label={PLAIN.detail} keys={[]} side="top" defaultOpen>
        <QuotaBar {...PLAIN} variant="foot" tabIndex={0} />
      </Tooltip>
    </div>
  ),
};
