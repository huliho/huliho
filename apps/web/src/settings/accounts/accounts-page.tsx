// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountList as Listed } from "@huliho/core";
import { accountsQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useRef } from "react";
import type { Ref, RefObject } from "react";

import buttonStyles from "../../design-system/button.module.css";
import { cx } from "../../design-system/cx";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import type { Locale } from "../../paraglide/runtime.js";
import { SettingsSection } from "../settings-section";
import { AccountList } from "./account-list";
import { useRemoveAccount } from "./use-remove-account";
import { useRetryAccount } from "./use-retry-account";
import styles from "./accounts-page.module.css";

// One or two accounts, the shape a personal list has.
const SKELETON_ROW_COUNT = 2;

interface AddAccountLinkProps {
  locale: Locale;
  ref?: Ref<HTMLAnchorElement> | undefined;
}

// The one way in to the card from here; a link, since it navigates.
export function AddAccountLink({ locale, ref }: AddAccountLinkProps) {
  return (
    <Link ref={ref} to="/accounts/new" className={cx(buttonStyles.button, buttonStyles.primary)}>
      {m.accounts_add({}, { locale })}
    </Link>
  );
}

interface ListedProps {
  locale: Locale;
  list: Listed;
  afterLast: RefObject<HTMLAnchorElement | null>;
}

function ListedAccounts({ locale, list, afterLast }: ListedProps) {
  const remove = useRemoveAccount(locale);
  const { outcomes, retry, forget } = useRetryAccount(locale);
  if (list.accounts.length === 0) {
    return <EmptyState message={m.account_lead({}, { locale })} />;
  }
  return (
    <AccountList
      rows={list.accounts}
      locale={locale}
      probeIntervalMinutes={list.probeIntervalMinutes}
      outcomes={outcomes}
      onRetry={retry}
      onRemove={(id) => {
        forget(id);
        remove(id);
      }}
      afterLast={afterLast}
    />
  );
}

export function AccountsPage() {
  const locale = getLocale();
  const query = useQuery(accountsQueryOptions);
  // The link stays in one place whether the rows or the empty sentence
  // show above it, so a removal of the last row can hand it the cursor.
  const add = useRef<HTMLAnchorElement>(null);
  const empty = query.isSuccess && query.data.accounts.length === 0;
  return (
    <SettingsSection title={m.accounts_heading({}, { locale })}>
      {query.isPending && <ListSkeleton locale={locale} rows={SKELETON_ROW_COUNT} />}
      {query.isError && (
        <ErrorState
          message={m.accounts_error({}, { locale })}
          retryLabel={m.retry_action({}, { locale })}
          onRetry={() => {
            void query.refetch();
          }}
        />
      )}
      {query.isSuccess && <ListedAccounts locale={locale} list={query.data} afterLast={add} />}
      <div className={cx(styles.footer, empty ? styles.centered : undefined)}>
        <AddAccountLink locale={locale} ref={add} />
      </div>
    </SettingsSection>
  );
}
