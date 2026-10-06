// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, MailCache } from "@huliho/core";
import { Moon, Sun } from "lucide-react";
import { Fragment, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Avatar } from "../design-system/avatar";
import { Button, focusHeir } from "../design-system/button";
import { cx } from "../design-system/cx";
import iconButton from "../design-system/icon-button.module.css";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { AttachmentStrip } from "./attachments/attachment-strip";
import { MessageBody } from "./message-body";
import {
  RECIPIENT_FIELDS,
  displayName,
  recipientNames,
  recipientsIn,
  senderOf,
} from "./recipients";
import type { RecipientField } from "./recipients";
import { RemoteContentBar } from "./remote-content-bar";
import { formatMessageTime, formatRowTime } from "./row-time";
import type { PlannedMessage } from "./thread-messages";
import { useMarkRead } from "./use-mark-read";
import { useOpenMessage } from "./use-open-message";
import type { Revert } from "./use-open-message";
import styles from "./message-card.module.css";

const FIELD_LABELS = new Map<RecipientField, (locale: Locale) => string>([
  ["to", (locale) => m.thread_field_to({}, { locale })],
  ["cc", (locale) => m.thread_field_cc({}, { locale })],
  ["bcc", (locale) => m.thread_field_bcc({}, { locale })],
  ["replyTo", (locale) => m.thread_field_reply_to({}, { locale })],
]);

interface MessageCardProps {
  locale: Locale;
  // The start of today, which a collapsed card's time reads against.
  today: number;
  message: PlannedMessage;
  expanded: boolean;
  cache: MailCache;
  accountId: string;
  onToggle: () => void;
}

interface HeadProps {
  locale: Locale;
  today: number;
  message: PlannedMessage;
  expanded: boolean;
  onToggle: () => void;
  // The icon buttons after the fold button; a folded card carries none.
  children?: ReactNode;
}

// Who wrote and when, as one button that folds the card. The name and
// the address are mail content, so each reads in its own direction.
function Head({ locale, today, message, expanded, onToggle, children }: HeadProps) {
  const { email, unread } = message;
  const sender = senderOf(email);
  const name = sender.name ?? m.list_no_sender({}, { locale });
  return (
    <div className={styles.head}>
      <button type="button" className={styles.fold} aria-expanded={expanded} onClick={onToggle}>
        <span className={styles.dot} aria-hidden="true" />
        <span className={styles.portrait}>
          <Avatar name={name} locale={locale} />
        </span>
        <span dir="auto" className={styles.name}>
          {name}
        </span>
        <span dir="auto" className={styles.detail}>
          {expanded ? sender.address : email.preview}
        </span>
        <time className={styles.time} dateTime={email.receivedAt}>
          {expanded
            ? formatMessageTime(email.receivedAt, locale)
            : formatRowTime(email.receivedAt, today, locale)}
        </time>
        {unread && <span className={spoken.spoken}>{m.thread_unread({}, { locale })}</span>}
      </button>
      {children}
    </div>
  );
}

// Shows a light-only message as it was sent and adapts it again. The
// pressed look says the message stands as sent.
function RevertButton({ locale, revert }: { locale: Locale; revert: Revert }) {
  return (
    <button
      type="button"
      className={cx(iconButton.button, styles.revert)}
      data-pressed={revert.original || undefined}
      aria-label={
        revert.original
          ? m.body_colors_adapt({}, { locale })
          : m.body_colors_original({}, { locale })
      }
      onClick={revert.toggle}
    >
      {revert.original ? (
        <Moon className={styles.headIcon} aria-hidden="true" />
      ) : (
        <Sun className={styles.headIcon} aria-hidden="true" />
      )}
    </button>
  );
}

interface DetailsProps {
  locale: Locale;
  email: EmailHeader;
}

// Every recipient by field, with the address beside a name.
function RecipientList({ locale, email }: DetailsProps) {
  const fields = RECIPIENT_FIELDS.map((field) => ({
    field,
    addresses: recipientsIn(email, field),
  }));
  return (
    <dl className={styles.list}>
      {fields
        .filter(({ addresses }) => addresses.length > 0)
        .map(({ field, addresses }) => (
          <Fragment key={field}>
            <dt>{FIELD_LABELS.get(field)?.(locale)}</dt>
            {addresses.map((address) => (
              <dd key={address.email} className={styles.address}>
                <span dir="auto">{displayName(address)}</span>
                {displayName(address) !== address.email && (
                  <span dir="auto" className={styles.muted}>
                    {address.email}
                  </span>
                )}
              </dd>
            ))}
          </Fragment>
        ))}
    </dl>
  );
}

// The recipients on one line with a button for all of them.
function Recipients({ locale, email }: DetailsProps) {
  const [shown, setShown] = useState(false);
  const names = recipientNames(email.to, locale);
  return (
    <div className={styles.details}>
      <div className={styles.recipients}>
        {names !== "" && (
          <span dir="auto" className={styles.toLine}>
            {m.thread_to({ names }, { locale })}
          </span>
        )}
        <Button
          variant="plain"
          aria-expanded={shown}
          onClick={() => {
            setShown(!shown);
          }}
        >
          {shown
            ? m.thread_recipients_hide({}, { locale })
            : m.thread_recipients_show({}, { locale })}
        </Button>
      </div>
      {shown && <RecipientList locale={locale} email={email} />}
    </div>
  );
}

type OpenCardProps = Omit<MessageCardProps, "expanded">;

// The open card: the head with the revert control, the recipients, the
// bar when the message names remote images, the body under the hairline
// and the strip of attachments under that. The body's box gives the
// frame its type and colors and holds the focus of a control that leaves.
function OpenCard({ locale, today, message, cache, accountId, onToggle }: OpenCardProps) {
  const { email } = message;
  const bodyRef = useRef<HTMLDivElement>(null);
  const open = useOpenMessage(bodyRef, { locale, cache, accountId, email });
  return (
    <>
      <Head locale={locale} today={today} message={message} expanded onToggle={onToggle}>
        {open.revert !== null && <RevertButton locale={locale} revert={open.revert} />}
      </Head>
      <Recipients locale={locale} email={email} />
      {open.bar !== null && (
        <div className={styles.bar}>
          <RemoteContentBar {...open.bar} />
        </div>
      )}
      <div
        ref={bodyRef}
        {...focusHeir}
        className={styles.body}
        data-strip={open.strip !== null || undefined}
      >
        <MessageBody locale={locale} view={open.view} />
      </div>
      {open.strip !== null && <AttachmentStrip locale={locale} attachments={open.strip} />}
    </>
  );
}

// One message of an open thread. A card that opens unread marks itself
// read; a folded card is its head alone.
export function MessageCard(props: MessageCardProps) {
  const { locale, today, message, expanded, cache, accountId, onToggle } = props;
  useMarkRead(cache, accountId, message.email, expanded);
  return (
    <li
      className={styles.card}
      data-expanded={expanded || undefined}
      data-unread={message.unread || undefined}
    >
      {expanded ? (
        <OpenCard
          locale={locale}
          today={today}
          message={message}
          cache={cache}
          accountId={accountId}
          onToggle={onToggle}
        />
      ) : (
        <Head
          locale={locale}
          today={today}
          message={message}
          expanded={false}
          onToggle={onToggle}
        />
      )}
    </li>
  );
}
