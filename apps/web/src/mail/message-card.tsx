// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "@huliho/core";
import { Fragment, useState } from "react";

import { Avatar } from "../design-system/avatar";
import { Button } from "../design-system/button";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import {
  RECIPIENT_FIELDS,
  displayName,
  recipientNames,
  recipientsIn,
  senderOf,
} from "./recipients";
import type { RecipientField } from "./recipients";
import { formatMessageTime, formatRowTime } from "./row-time";
import type { PlannedMessage } from "./thread-messages";
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
  onToggle: () => void;
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

// The recipients on one line with a button for all of them, then the
// message's text: its preview sentence, the one sentence that stands
// for the body.
function MessageDetails({ locale, email }: DetailsProps) {
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
      {email.preview !== "" && (
        <p dir="auto" className={styles.text}>
          {email.preview}
        </p>
      )}
    </div>
  );
}

// One message of an open thread: its head is a button that folds the
// card; open, the card shows who got it and its text. Names, addresses
// and text are mail content, so each reads in its own direction.
export function MessageCard({ locale, today, message, expanded, onToggle }: MessageCardProps) {
  const { email, unread } = message;
  const sender = senderOf(email);
  const name = sender.name ?? m.list_no_sender({}, { locale });
  return (
    <li
      className={styles.card}
      data-expanded={expanded || undefined}
      data-unread={unread || undefined}
    >
      <button type="button" className={styles.head} aria-expanded={expanded} onClick={onToggle}>
        <span className={styles.dot} aria-hidden="true" />
        <Avatar name={name} locale={locale} />
        <span className={styles.who}>
          <span dir="auto" className={styles.name}>
            {name}
          </span>
          <span dir="auto" className={styles.detail}>
            {expanded ? sender.address : email.preview}
          </span>
        </span>
        <time className={styles.time} dateTime={email.receivedAt}>
          {expanded
            ? formatMessageTime(email.receivedAt, locale)
            : formatRowTime(email.receivedAt, today, locale)}
        </time>
        {unread && <span className={spoken.spoken}>{m.thread_unread({}, { locale })}</span>}
      </button>
      {expanded && <MessageDetails locale={locale} email={email} />}
    </li>
  );
}
