// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { RemoteContentBar } from "./remote-content-bar";
import type { RemoteContentBarProps } from "./remote-content-bar";
import styles from "./message-card.module.css";

const SENDER = "redactie@koersbrief.example";

function nothing(): void {
  // A drawn bar changes nothing.
}

// The bar in the chrome of a card, above where the body would start.
function Bar(props: Partial<RemoteContentBarProps>): JSX.Element {
  return (
    <ol role="list" style={{ margin: 0, padding: 0, listStyle: "none" }}>
      <li className={styles.card} data-expanded>
        <div className={styles.bar} style={{ paddingBlockStart: "var(--hhx-space-3)" }}>
          <RemoteContentBar
            locale="en"
            state="blocked"
            sender={SENDER}
            canAlways
            pending={false}
            onLoadOnce={nothing}
            onAllow={nothing}
            onStop={nothing}
            {...props}
          />
        </div>
      </li>
    </ol>
  );
}

const meta: Meta = {
  title: "Mail/RemoteContentBar",
};

export default meta;

export const Blocked: StoryObj = {
  render: () => <Bar />,
};

export const BlockedLoadOnceAlone: StoryObj = {
  render: () => <Bar canAlways={false} />,
};

export const LoadedOnce: StoryObj = {
  render: () => <Bar state="once" />,
};

export const Always: StoryObj = {
  render: () => <Bar state="always" />,
};

export const FailedCheck: StoryObj = {
  render: () => <Bar state="failed" canAlways={false} />,
};
