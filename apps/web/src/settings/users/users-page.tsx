// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { grantableRoles } from "@huliho/core";
import { sessionQueryOptions, usersQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";

import { Button } from "../../design-system/button";
import { ErrorState } from "../../design-system/error-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import { SettingsSection } from "../settings-section";
import { useUserFlow } from "./use-user-flow";
import { UserDialog } from "./user-dialog";
import { UserTable } from "./user-table";

// The actor plus two colleagues, the shape a small organization has.
const SKELETON_ROW_COUNT = 3;

export function UsersPage() {
  const locale = getLocale();
  const actor = useQuery(sessionQueryOptions).data?.user;
  const query = useQuery(usersQueryOptions);
  const flow = useUserFlow(locale);
  // The route guard fetched the session; without one there is nothing to show.
  if (actor === undefined) {
    return null;
  }
  const createButton = (
    <Button
      variant="primary"
      onClick={() => {
        flow.start({ kind: "create" });
      }}
    >
      {m.users_create({}, { locale })}
    </Button>
  );
  return (
    <>
      <SettingsSection title={m.users_heading({}, { locale })} action={createButton}>
        {query.isPending && <ListSkeleton locale={locale} rows={SKELETON_ROW_COUNT} />}
        {query.isError && (
          <ErrorState
            message={m.users_error({}, { locale })}
            retryLabel={m.retry_action({}, { locale })}
            onRetry={() => {
              void query.refetch();
            }}
          />
        )}
        {query.isSuccess && (
          <UserTable
            rows={query.data}
            locale={locale}
            // Relative times count from the fetch, so a refetch refreshes them.
            now={new Date(query.dataUpdatedAt)}
            actor={actor}
            onReset={(user) => {
              flow.start({ kind: "reset", user });
            }}
          />
        )}
      </SettingsSection>
      <UserDialog locale={locale} roles={grantableRoles(actor.role)} flow={flow} />
    </>
  );
}
