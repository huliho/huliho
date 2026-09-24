// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import type { CSSProperties, ReactNode } from "react";

import { ErrorState } from "../design-system/error-state";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { PaneBoundary } from "../shell/pane-boundary";
import { PaneHeader } from "./pane-header";
import type { PanePosition } from "./thread-pane";
import type { TreeState } from "./tree";
import styles from "./list-pane.module.css";

interface ListPaneProps {
  locale: Locale;
  // Where the reading pane stands, which decides which way the list is sized.
  position: PanePosition;
  // The id the seam names as the pane it sizes.
  id: string;
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
  // The list's width beside the pane or its height above it, in CSS
  // pixels; null where the list fills the screen.
  size: number | null;
  // While a thread stands over the list as a screen, the list is out of reach.
  inert: boolean;
  onOpenSidebar?: (() => void) | undefined;
  children: ReactNode;
}

function sized(position: PanePosition, size: number | null): CSSProperties | undefined {
  if (size === null) {
    return undefined;
  }
  const length = `${String(size)}px`;
  return position === "bottom" ? { blockSize: length } : { inlineSize: length };
}

// The list pane: the mailbox header over the route's body, or over the
// error state when the tree did not load. A render failure in the body
// stays inside it.
export function ListPane(props: ListPaneProps) {
  const { locale, tree, position, size } = props;
  const mailbox =
    tree.status === "success"
      ? tree.mailboxes.find((row) => row.id === props.currentMailboxId)
      : undefined;
  return (
    <main
      id={props.id}
      className={styles.main}
      data-pane={position}
      inert={props.inert || undefined}
      style={sized(position, size)}
    >
      <PaneHeader
        locale={locale}
        account={props.account}
        mailbox={mailbox}
        pending={tree.status === "pending"}
        onOpenSidebar={props.onOpenSidebar}
      />
      <div className={styles.body}>
        {tree.status === "error" && (
          <ErrorState
            message={m.mail_error({}, { locale })}
            retryLabel={m.retry_action({}, { locale })}
            onRetry={tree.retry}
          />
        )}
        {tree.status === "success" && <PaneBoundary>{props.children}</PaneBoundary>}
      </div>
    </main>
  );
}
