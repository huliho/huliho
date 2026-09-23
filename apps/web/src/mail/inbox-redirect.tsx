// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { mailboxesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Navigate, useParams } from "@tanstack/react-router";

import { mailCache } from "../cache/client";
import { EmptyState } from "../design-system/empty-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import { buildTree, landingMailbox } from "./tree";

// An account opened without a mailbox goes to its inbox, or to the first
// mailbox it has.
export function InboxRedirect() {
  const locale = useLocale();
  const { accountId } = useParams({ from: "/signed-in/mail/$accountId/" });
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  if (!tree.isSuccess) {
    return null;
  }
  const target = landingMailbox(buildTree(tree.data, locale));
  if (target === null) {
    return <EmptyState message={m.mail_no_mailboxes({}, { locale })} />;
  }
  return (
    <Navigate
      to="/mail/$accountId/$mailboxId"
      params={{ accountId, mailboxId: target.id }}
      replace
    />
  );
}
