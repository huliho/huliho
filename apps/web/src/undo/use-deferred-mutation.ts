// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import type { QueryKey } from "@tanstack/react-query";

import { toastManager } from "../design-system/toast";
import { trackPending } from "./pending";

interface DeferredMutation<TRow, TVariables> {
  // The cached list the rows leave at once and return to on undo.
  queryKey: QueryKey;
  keep: (row: TRow, variables: TVariables) => boolean;
  mutate: (variables: TVariables, options: { keepalive: boolean }) => Promise<void>;
  message: (removed: TRow[]) => string;
  failureMessage: string;
}

// Removes rows from a cached list right away and sends the mutation only
// once the undo toast has run out; undo puts the rows back and sends nothing.
export function useDeferredMutation<TRow, TVariables>(
  definition: DeferredMutation<TRow, TVariables>,
): (variables: TVariables) => void {
  const queryClient = useQueryClient();
  return (variables) => {
    const previous = queryClient.getQueryData<TRow[]>(definition.queryKey) ?? [];
    const removed = previous.filter((row) => !definition.keep(row, variables));
    if (removed.length === 0) {
      return;
    }
    const apply = (): void => {
      queryClient.setQueryData<TRow[]>(definition.queryKey, (rows) =>
        rows?.filter((row) => definition.keep(row, variables)),
      );
    };
    apply();
    let settled = false;
    let toastId = "";
    let untrack: (() => void) | undefined;
    const flush = (keepalive: boolean): void => {
      if (settled) {
        return;
      }
      settled = true;
      untrack?.();
      toastManager.close(toastId);
      // The server answers with the truth on failure; the other pending
      // removals apply themselves again on that answer.
      definition.mutate(variables, { keepalive }).catch(() => {
        void queryClient.invalidateQueries({ queryKey: definition.queryKey });
        toastManager.add({ description: definition.failureMessage });
      });
    };
    // Undo puts back only its own rows, at their former places, so a
    // removal still pending next to it stays out of the list.
    const restore = (rows: TRow[] = []): TRow[] => {
      const result = [...rows];
      for (const row of removed) {
        result.splice(Math.min(previous.indexOf(row), result.length), 0, row);
      }
      return result;
    };
    const undo = (): void => {
      if (settled) {
        return;
      }
      settled = true;
      untrack?.();
      queryClient.setQueryData<TRow[]>(definition.queryKey, restore);
      toastManager.close(toastId);
    };
    untrack = trackPending({ flush, reapply: apply });
    toastId = toastManager.add({
      description: definition.message(removed),
      data: { undo },
      actionProps: { onClick: undo },
      onClose: () => {
        flush(false);
      },
    });
  };
}
