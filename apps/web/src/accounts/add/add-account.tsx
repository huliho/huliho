// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys, sessionQueryOptions } from "@huliho/state";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLoaderData, useNavigate } from "@tanstack/react-router";

import { toastManager } from "../../design-system/toast";
import { useLocale } from "../../i18n/locale";
import { m } from "../../paraglide/messages.js";
import { ShellHeader } from "../../shell/shell-header";
import { useOnline } from "../../shell/use-online";
import { AddAccountCard } from "./add-account-card";
import { useAddAccount } from "./use-add-account";
import styles from "./add-account.module.css";

// The card's page: the shell bar above it keeps Settings and sign-out
// reachable; a connect lands on the accounts page with a toast.
export function AddAccount() {
  const locale = useLocale();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const account = useLoaderData({ from: "/signed-in/accounts/new" });
  const online = useOnline();
  // The route guard read the session already; the providers ride along.
  const signInProviders = useQuery(sessionQueryOptions).data?.signInProviders ?? [];
  const flow = useAddAccount(locale, account, (name) => {
    // The shell guard reads the list; a stale one would send the session back here.
    void queryClient.invalidateQueries({ queryKey: queryKeys.accounts });
    toastManager.add({ description: m.account_connected_toast({ name }, { locale }) });
    void navigate({ to: "/settings/accounts" });
  });
  return (
    <div className={styles.page}>
      <ShellHeader locale={locale} />
      <main className={styles.screen}>
        <AddAccountCard
          locale={locale}
          flow={flow}
          online={online}
          signInProviders={signInProviders}
        />
      </main>
    </div>
  );
}
