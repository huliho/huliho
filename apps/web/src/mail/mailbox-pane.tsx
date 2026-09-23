// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { mailboxesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Link, Navigate, useParams } from "@tanstack/react-router";

import { mailCache } from "../cache/client";
import buttonStyles from "../design-system/button.module.css";
import { cx } from "../design-system/cx";
import { EmptyState } from "../design-system/empty-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./mailbox-pane.module.css";

interface EmptyMailboxProps {
  locale: Locale;
  accountId: string;
  mailbox: Mailbox;
  mailboxes: readonly Mailbox[];
}

// An empty inbox points at the archive, any other empty mailbox at the
// inbox; the link stays away when the account has no such mailbox.
export function EmptyMailbox({ locale, accountId, mailbox, mailboxes }: EmptyMailboxProps) {
  const inbox = mailbox.role === "inbox";
  const target = mailboxes.find((row) => row.role === (inbox ? "archive" : "inbox"));
  return (
    <div className={styles.empty}>
      <EmptyState
        message={
          inbox
            ? m.mail_empty_inbox({}, { locale })
            : m.mail_empty_mailbox({ mailbox: mailbox.name }, { locale })
        }
      />
      {target !== undefined && (
        <Link
          to="/mail/$accountId/$mailboxId"
          params={{ accountId, mailboxId: target.id }}
          className={cx(buttonStyles.button, buttonStyles.secondary)}
        >
          {inbox ? m.mail_open_archive({}, { locale }) : m.mail_open_inbox({}, { locale })}
        </Link>
      )}
    </div>
  );
}

// The list pane of one mailbox. A mailbox the tree lacks goes back to
// the account's inbox; an empty one says so.
export function MailboxPane() {
  const locale = useLocale();
  const { accountId, mailboxId } = useParams({ from: "/signed-in/mail/$accountId/$mailboxId" });
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  if (!tree.isSuccess) {
    return null;
  }
  const mailbox = tree.data.find((row) => row.id === mailboxId);
  if (mailbox === undefined) {
    return <Navigate to="/mail/$accountId" params={{ accountId }} replace />;
  }
  if (mailbox.totalEmails > 0) {
    return null;
  }
  return (
    <EmptyMailbox locale={locale} accountId={accountId} mailbox={mailbox} mailboxes={tree.data} />
  );
}
