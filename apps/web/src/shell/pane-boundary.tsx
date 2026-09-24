// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Component } from "react";
import type { ReactNode } from "react";

import { ErrorState } from "../design-system/error-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";

interface BoundaryProps {
  message: string;
  retryLabel: string;
  children: ReactNode;
}

interface BoundaryState {
  failed: boolean;
}

// A render failure below stays inside this pane; Try again renders it
// afresh. A class, since only one can catch a render error.
class Boundary extends Component<BoundaryProps, BoundaryState> {
  override state: BoundaryState = { failed: false };

  static getDerivedStateFromError(): BoundaryState {
    return { failed: true };
  }

  override componentDidCatch(error: Error): void {
    console.error("pane: a render failed", error.name);
  }

  override render(): ReactNode {
    if (!this.state.failed) {
      return this.props.children;
    }
    return (
      <ErrorState
        message={this.props.message}
        retryLabel={this.props.retryLabel}
        onRetry={() => {
          this.setState({ failed: false });
        }}
      />
    );
  }
}

// Wraps one pane of the mail screen: a failure in it shows the error
// sentence with Try again there and leaves the other panes standing.
export function PaneBoundary({ children }: { children: ReactNode }) {
  const locale = useLocale();
  return (
    <Boundary message={m.pane_error({}, { locale })} retryLabel={m.retry_action({}, { locale })}>
      {children}
    </Boundary>
  );
}
