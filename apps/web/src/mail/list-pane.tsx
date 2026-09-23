// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import type { ReactNode } from "react";

import { ErrorState } from "../design-system/error-state";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { Layout } from "../shell/breakpoints";
import { PaneHeader } from "./pane-header";
import type { TreeState } from "./tree";
import styles from "./list-pane.module.css";

interface ListPaneProps {
  locale: Locale;
  layout: Layout;
  // The id the seam names as the pane it sizes.
  id: string;
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
  // The list's width in CSS pixels; null on a phone, where the list fills the screen.
  width: number | null;
  onOpenSidebar?: (() => void) | undefined;
  children: ReactNode;
}

// The list pane: the mailbox header over the route's body, or over the
// error state when the tree did not load.
export function ListPane(props: ListPaneProps) {
  const { locale, tree, width } = props;
  const mailbox =
    tree.status === "success"
      ? tree.mailboxes.find((row) => row.id === props.currentMailboxId)
      : undefined;
  return (
    <main
      id={props.id}
      className={styles.main}
      data-layout={props.layout}
      style={width === null ? undefined : { inlineSize: `${String(width)}px` }}
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
        {tree.status === "success" && props.children}
      </div>
    </main>
  );
}
