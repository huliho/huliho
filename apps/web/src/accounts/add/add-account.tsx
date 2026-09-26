// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys, sessionQueryOptions } from "@huliho/state";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLoaderData, useLocation, useNavigate } from "@tanstack/react-router";

import { toastManager } from "../../design-system/toast";
import { useLocale } from "../../i18n/locale";
import { openedFromMailbox } from "../../mail/thread-history";
import { m } from "../../paraglide/messages.js";
import { ShellHeader } from "../../shell/shell-header";
import { useOnline } from "../../shell/use-online";
import { AddAccountCard } from "./add-account-card";
import { useAddAccount } from "./use-add-account";
import styles from "./add-account.module.css";

// The card's page: the shell bar above it keeps Settings and sign-out
// reachable. A new account opens in its inbox with a toast; a
// reconnect returns where it came from, the mail when the banner sent
// it and the accounts page otherwise.
export function AddAccount() {
  const locale = useLocale();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const account = useLoaderData({ from: "/signed-in/accounts/new" });
  const fromMailbox = useLocation({ select: (location) => openedFromMailbox(location.state) });
  const online = useOnline();
  // The route guard read the session already; the providers ride along.
  const signInProviders = useQuery(sessionQueryOptions).data?.signInProviders ?? [];
  const flow = useAddAccount(locale, account, (id, name) => {
    // The shell guard reads the list; a stale one would not hold the row.
    void queryClient.invalidateQueries({ queryKey: queryKeys.accounts });
    toastManager.add({ description: m.account_connected_toast({ name }, { locale }) });
    if (account === null || fromMailbox) {
      void navigate({ to: "/mail/$accountId", params: { accountId: id } });
    } else {
      void navigate({ to: "/settings/accounts" });
    }
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
