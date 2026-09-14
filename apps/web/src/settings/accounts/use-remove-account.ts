// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { removeAccount } from "@huliho/core";
import type { AccountList, AccountRow } from "@huliho/core";
import { queryKeys } from "@huliho/state";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { useDeferredMutation } from "../../undo/use-deferred-mutation";

// Remove behind the undo toast: the row leaves the list at once and the
// request goes out when the toast has run out. The mail stays at the
// provider, so nothing asks first.
export function useRemoveAccount(locale: Locale): (id: string) => void {
  return useDeferredMutation<AccountList, AccountRow, string>({
    queryKey: queryKeys.accounts,
    rows: (list) => list.accounts,
    withRows: (list, accounts) => ({ ...list, accounts }),
    keep: (row, id) => row.id !== id,
    mutate: (id, options) => removeAccount(id, options),
    message: (removed) => m.accounts_removed_toast({ name: removed[0]?.name ?? "" }, { locale }),
    failureMessage: m.accounts_remove_failed({}, { locale }),
  });
}
