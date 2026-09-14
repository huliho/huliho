// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import type { QueryClient, QueryKey } from "@tanstack/react-query";

import { toastManager } from "../design-system/toast";
import { trackPending } from "./pending";

// Where the rows sit in the cached answer and how a changed list goes back.
interface RowLens<TData, TRow> {
  rows: (data: TData) => TRow[];
  withRows: (data: TData, rows: TRow[]) => TData;
}

interface DeferredMutation<TData, TRow, TVariables> extends RowLens<TData, TRow> {
  // The cached answer the rows leave at once and return to on undo.
  queryKey: QueryKey;
  keep: (row: TRow, variables: TVariables) => boolean;
  mutate: (variables: TVariables, options: { keepalive: boolean }) => Promise<void>;
  message: (removed: TRow[]) => string;
  failureMessage: string;
}

interface Removal<TRow> {
  removed: TRow[];
  // Takes the rows out of the cache; runs again when fresh data puts them back.
  apply: () => void;
  // Puts back only these rows, at their former places, so a removal still
  // pending next to them stays out of the list.
  restore: () => void;
}

// A cached answer that is the list itself.
export function listLens<TRow>(): RowLens<TRow[], TRow> {
  return { rows: (list) => list, withRows: (_list, rows) => rows };
}

function removalOf<TData, TRow, TVariables>(
  queryClient: QueryClient,
  definition: DeferredMutation<TData, TRow, TVariables>,
  variables: TVariables,
): Removal<TRow> {
  const cached = queryClient.getQueryData<TData>(definition.queryKey);
  const previous = cached === undefined ? [] : definition.rows(cached);
  const removed = previous.filter((row) => !definition.keep(row, variables));
  // An evicted cache stays as it is; the next fetch tells the truth.
  const write = (change: (rows: TRow[]) => TRow[]): void => {
    queryClient.setQueryData<TData>(definition.queryKey, (data) =>
      data === undefined ? undefined : definition.withRows(data, change(definition.rows(data))),
    );
  };
  return {
    removed,
    apply: () => {
      write((rows) => rows.filter((row) => definition.keep(row, variables)));
    },
    restore: () => {
      write((rows) => {
        const result = [...rows];
        for (const row of removed) {
          result.splice(Math.min(previous.indexOf(row), result.length), 0, row);
        }
        return result;
      });
    },
  };
}

// Removes rows from a cached answer right away and sends the mutation only
// once the undo toast has run out; undo puts the rows back and sends nothing.
export function useDeferredMutation<TData, TRow, TVariables>(
  definition: DeferredMutation<TData, TRow, TVariables>,
): (variables: TVariables) => void {
  const queryClient = useQueryClient();
  return (variables) => {
    const removal = removalOf(queryClient, definition, variables);
    if (removal.removed.length === 0) {
      return;
    }
    removal.apply();
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
    const undo = (): void => {
      if (settled) {
        return;
      }
      settled = true;
      untrack?.();
      removal.restore();
      toastManager.close(toastId);
    };
    untrack = trackPending({ flush, reapply: removal.apply });
    toastId = toastManager.add({
      description: definition.message(removal.removed),
      data: { undo },
      actionProps: { onClick: undo },
      onClose: () => {
        flush(false);
      },
    });
  };
}
