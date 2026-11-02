// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { hideWrite } from "@huliho/core";
import type { Mailbox } from "@huliho/core";
import { accountsQueryOptions, mailboxesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";
import { Navigate, useParams } from "@tanstack/react-router";

import { mailCache } from "../../cache/client";
import { useMailCache } from "../../cache/use-mail-cache";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { useLocale } from "../../i18n/locale";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { useOnline } from "../../shell/use-online";
import { SettingsSection } from "../settings-section";
import {
  ROLES,
  closedFolders,
  hiddenFolders,
  roleRows,
  roleWrite,
  showWrite,
} from "./folder-choices";
import {
  ClosedSection,
  HiddenSection,
  RolesSection,
  closedControl,
  roleControl,
  showControl,
} from "./folder-sections";
import { useFolderWrites } from "./use-folder-writes";

// One still row per role.
const SKELETON_ROW_COUNT = ROLES.length;

interface LoadedProps {
  locale: Locale;
  accountId: string;
  name: string;
  mailboxes: Mailbox[];
}

function LoadedFolders({ locale, accountId, name, mailboxes }: LoadedProps) {
  const online = useOnline();
  const { pending, send, settle } = useFolderWrites(mailCache, accountId, locale);
  const closed = closedFolders(mailboxes, locale);
  return (
    <>
      <RolesSection
        locale={locale}
        name={name}
        rows={roleRows(mailboxes, locale)}
        online={online}
        pending={pending}
        onRole={(role, takerId) => {
          settle(roleControl(role), takerId ?? "", roleWrite(mailboxes, role, takerId));
        }}
      />
      <HiddenSection
        locale={locale}
        folders={hiddenFolders(mailboxes, locale)}
        online={online}
        pending={pending}
        onShow={(id) => {
          send(showControl(id), id, showWrite(id), m.folder_done({ kind: "shown" }, { locale }));
        }}
      />
      {closed.length > 0 && (
        <ClosedSection
          locale={locale}
          folders={closed}
          online={online}
          pending={pending}
          onClosed={(id, choice) => {
            const write = choice === "hide" ? hideWrite(mailboxes, id) : showWrite(id);
            send(closedControl(id), choice, write);
          }}
        />
      )}
    </>
  );
}

interface StateProps {
  locale: Locale;
  query: UseQueryResult<Mailbox[]>;
}

// What stands while the tree is not there: still rows, then the error with
// Try again or the sentence of an account that holds no mailbox yet.
function FoldersState({ locale, query }: StateProps) {
  if (query.isPending) {
    return <ListSkeleton locale={locale} rows={SKELETON_ROW_COUNT} />;
  }
  if (query.isError) {
    return (
      <ErrorState
        variant="inline"
        message={m.folders_error({}, { locale })}
        retryLabel={m.retry_action({}, { locale })}
        onRetry={() => {
          void query.refetch();
        }}
      />
    );
  }
  return <EmptyState message={m.mail_no_mailboxes({}, { locale })} />;
}

// Where the account's six roles live, which folders the tree leaves out
// and what a mailbox the server does not open does in the tree.
export function FoldersPage() {
  const locale = useLocale();
  const { accountId } = useParams({ from: "/signed-in/settings/accounts/$accountId/folders" });
  // A visit by its address starts the worker no mail screen started.
  useMailCache(null);
  const list = useQuery(accountsQueryOptions);
  const query = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const name = list.data?.accounts.find((row) => row.id === accountId)?.name;
  if (name === undefined) {
    // The account left the session: the list is the nearest page that stands.
    return list.data === undefined ? null : <Navigate to="/settings/accounts" replace />;
  }
  if (query.isSuccess && query.data.length > 0) {
    return (
      <LoadedFolders locale={locale} accountId={accountId} name={name} mailboxes={query.data} />
    );
  }
  return (
    <SettingsSection title={name}>
      <FoldersState locale={locale} query={query} />
    </SettingsSection>
  );
}
