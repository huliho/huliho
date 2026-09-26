// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Button } from "../design-system/button";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

interface RetryButtonProps {
  locale: Locale;
  // The account's name, which the label reads out.
  name: string;
  pending: boolean;
  onRetry: () => void;
}

// Retry for a stopped account, busy while the retry runs.
export function RetryButton({ locale, name, pending, onRetry }: RetryButtonProps) {
  return (
    <Button
      aria-label={
        pending
          ? m.accounts_retrying_for({ name }, { locale })
          : m.accounts_retry_for({ name }, { locale })
      }
      pending={pending}
      onClick={onRetry}
    >
      {pending ? m.accounts_retrying({}, { locale }) : m.accounts_retry({}, { locale })}
    </Button>
  );
}
