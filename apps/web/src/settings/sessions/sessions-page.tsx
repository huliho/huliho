// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SessionRow } from "@huliho/core";
import { revokeOtherSessions, revokeSession } from "@huliho/core";
import { queryKeys, sessionsQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";

import { ErrorState } from "../../design-system/error-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import { useDeferredMutation } from "../../undo/use-deferred-mutation";
import { SettingsSection } from "../settings-section";
import { revokedToast } from "./device-label";
import { PasswordSection } from "./password-section";
import { SessionList } from "./session-list";

// The current device plus two typical others, the shape the list usually has.
const SKELETON_ROW_COUNT = 3;

export function SessionsPage() {
  const locale = getLocale();
  const query = useQuery(sessionsQueryOptions);
  const revoke = useDeferredMutation<SessionRow, string>({
    queryKey: queryKeys.sessions,
    keep: (row, id) => row.id !== id,
    mutate: (id, options) => revokeSession(id, options),
    message: (removed) => revokedToast(removed[0]?.device, locale),
    failureMessage: m.sessions_revoke_failed({}, { locale }),
  });
  const revokeOthers = useDeferredMutation<SessionRow, null>({
    queryKey: queryKeys.sessions,
    keep: (row) => row.current,
    mutate: (_variables, options) => revokeOtherSessions(options),
    message: (removed) => m.sessions_revoked_others_toast({ count: removed.length }, { locale }),
    failureMessage: m.sessions_revoke_failed({}, { locale }),
  });
  return (
    <>
      <SettingsSection title={m.sessions_heading({}, { locale })}>
        {query.isPending && <ListSkeleton locale={locale} rows={SKELETON_ROW_COUNT} />}
        {query.isError && (
          <ErrorState
            message={m.sessions_error({}, { locale })}
            retryLabel={m.retry_action({}, { locale })}
            onRetry={() => {
              void query.refetch();
            }}
          />
        )}
        {query.isSuccess && (
          <SessionList
            rows={query.data}
            locale={locale}
            // Relative times count from the fetch, so a refetch refreshes them.
            now={new Date(query.dataUpdatedAt)}
            onRevoke={revoke}
            onRevokeOthers={() => {
              revokeOthers(null);
            }}
          />
        )}
      </SettingsSection>
      <PasswordSection />
    </>
  );
}
