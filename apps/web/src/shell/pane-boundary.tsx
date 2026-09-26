// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Component, createContext, use } from "react";
import type { ReactNode } from "react";

import { ErrorState } from "../design-system/error-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";

// Renders the boundary afresh; set while it shows its error state.
const RetryContext = createContext<(() => void) | null>(null);

interface PaneBoundaryProps {
  // What holds the error state: a pane shows it bare, a popup puts it in
  // a layer of its own, with `PaneErrorState` inside.
  frame?: ReactNode | undefined;
  children: ReactNode;
}

interface BoundaryState {
  failed: boolean;
}

// The error sentence with Try again, inside a boundary's frame.
export function PaneErrorState() {
  const locale = useLocale();
  const retry = use(RetryContext);
  if (retry === null) {
    return null;
  }
  return (
    <ErrorState
      message={m.pane_error({}, { locale })}
      retryLabel={m.retry_action({}, { locale })}
      onRetry={retry}
    />
  );
}

// Wraps one pane of the mail screen: a failure in it shows the error
// sentence with Try again there and leaves the other panes standing. A
// class, since only one can catch a render error.
export class PaneBoundary extends Component<PaneBoundaryProps, BoundaryState> {
  override state: BoundaryState = { failed: false };

  static getDerivedStateFromError(): BoundaryState {
    return { failed: true };
  }

  override componentDidCatch(error: Error): void {
    console.error("pane: a render failed", error.name);
  }

  private readonly retry = (): void => {
    this.setState({ failed: false });
  };

  override render(): ReactNode {
    if (!this.state.failed) {
      return this.props.children;
    }
    return <RetryContext value={this.retry}>{this.props.frame ?? <PaneErrorState />}</RetryContext>;
  }
}
