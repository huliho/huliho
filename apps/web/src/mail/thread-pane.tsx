// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, Mailbox, ThreadDetail } from "@huliho/core";
import { threadQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, X } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";

import { KeyCaps } from "../commands/key-caps";
import { ESCAPE } from "../commands/keys";
import type { Chord } from "../commands/keys";
import { useCommand } from "../commands/use-command";
import { Button } from "../design-system/button";
import { cx } from "../design-system/cx";
import { EmptyState } from "../design-system/empty-state";
import { ErrorState } from "../design-system/error-state";
import iconButton from "../design-system/icon-button.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { MessageCard } from "./message-card";
import { messagesOf, planMessages, subjectOf } from "./thread-messages";
import { ThreadSkeleton } from "./thread-skeleton";
import { useToday } from "./use-today";
import styles from "./thread-pane.module.css";

// Where the pane stands: beside the list, below it or as a screen of its own.
export type PanePosition = "right" | "bottom" | "screen";

// The key that closes the thread, shown beside the way back.
const CLOSE_KEYS: readonly Chord[] = [ESCAPE];

export interface ThreadPaneProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  threadId: string;
  // The mailbox the thread was opened from, which the way back names.
  mailbox: Mailbox | undefined;
  position: PanePosition;
  // Whether the way back shows its key; a phone has none.
  keyHints: boolean;
  onClose: () => void;
}

interface ToolbarProps {
  locale: Locale;
  position: PanePosition;
  mailbox: Mailbox | undefined;
  keyHints: boolean;
  onClose: () => void;
}

// Beside or below the list a Close button heads the pane; on a screen
// of its own the way back to the mailbox does.
function Toolbar({ locale, position, mailbox, keyHints, onClose }: ToolbarProps) {
  if (position !== "screen") {
    return (
      <div className={styles.toolbar}>
        <button
          type="button"
          className={iconButton.button}
          aria-label={m.thread_close({}, { locale })}
          onClick={onClose}
        >
          <X className={iconButton.icon} aria-hidden="true" />
        </button>
      </div>
    );
  }
  const name = mailbox?.name ?? m.thread_back_unnamed({}, { locale });
  const label =
    mailbox === undefined
      ? m.thread_back_unnamed({}, { locale })
      : m.thread_back({ mailbox: mailbox.name }, { locale });
  return (
    <div className={styles.toolbar} data-position="screen">
      <button type="button" className={styles.back} aria-label={label} onClick={onClose}>
        <ChevronLeft className={cx(iconButton.icon, styles.chevron)} aria-hidden="true" />
        <span>{name}</span>
        {keyHints && <KeyCaps keys={CLOSE_KEYS} spoken={false} />}
      </button>
    </div>
  );
}

interface BodyProps {
  locale: Locale;
  today: number;
  detail: ThreadDetail;
  position: PanePosition;
  titleRef: RefObject<HTMLHeadingElement | null>;
}

// The cards the user folded the other way from how the pane drew them.
function useFlipped(): [ReadonlySet<string>, (id: string) => void] {
  const [flipped, setFlipped] = useState<ReadonlySet<string>>(new Set());
  const toggle = (id: string): void => {
    setFlipped((held) => {
      const next = new Set(held);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });
  };
  return [flipped, toggle];
}

// The subject, the count and one card per message, oldest first: the
// newest and the unread ones open, the rest collapsed, the oldest
// behind a button. A card toggles on its head.
function ThreadBody({ locale, today, detail, position, titleRef }: BodyProps) {
  const messages = messagesOf(detail);
  const plan = planMessages(messages);
  const [olderShown, setOlderShown] = useState(false);
  const [flipped, toggle] = useFlipped();
  const cardsRef = useRef<HTMLOListElement>(null);
  const revealedRef = useRef(false);
  // The button that revealed the older cards leaves; the first of them takes its focus.
  useLayoutEffect(() => {
    if (revealedRef.current) {
      revealedRef.current = false;
      cardsRef.current?.querySelector("button")?.focus();
    }
  });
  const Title = position === "screen" ? "h1" : "h2";
  return (
    <div className={cx(styles.thread, styles.reveal)}>
      {/* The subject is mail text: it reads in its own direction, not the UI's. */}
      <Title ref={titleRef} tabIndex={-1} dir="auto" className={styles.title}>
        {subjectOf(messages) ?? m.list_no_subject({}, { locale })}
      </Title>
      <p className={styles.count}>
        {m.thread_count({ count: detail.thread.emailIds.length }, { locale })}
      </p>
      {!olderShown && plan.olderCount > 0 && (
        <Button
          className={styles.older}
          onClick={() => {
            revealedRef.current = true;
            setOlderShown(true);
          }}
        >
          {m.thread_show_older({ count: plan.olderCount }, { locale })}
        </Button>
      )}
      {/* Safari drops the list semantics of a list without markers; the role keeps them. */}
      <ol ref={cardsRef} role="list" className={styles.cards}>
        {plan.messages
          .filter((message) => olderShown || !message.older)
          .map((message) => (
            <MessageCard
              key={message.email.id}
              locale={locale}
              today={today}
              message={message}
              expanded={flipped.has(message.email.id) !== message.expanded}
              onToggle={() => {
                toggle(message.email.id);
              }}
            />
          ))}
      </ol>
    </div>
  );
}

// One open thread: its toolbar over the cards. Escape closes it from
// anywhere; the title takes the focus once the thread is in.
export function ThreadPane(props: ThreadPaneProps) {
  const { locale, cache, accountId, threadId, mailbox, position, keyHints, onClose } = props;
  const today = useToday();
  const query = useQuery(threadQueryOptions(cache, accountId, threadId));
  const titleRef = useRef<HTMLHeadingElement>(null);
  const focusedForRef = useRef<string | null>(null);
  useCommand({
    id: "thread.close",
    label: m.thread_close({}, { locale }),
    group: "navigate",
    keys: CLOSE_KEYS,
    run: onClose,
  });
  useLayoutEffect(() => {
    const title = titleRef.current;
    if (title === null || focusedForRef.current === threadId) {
      return;
    }
    focusedForRef.current = threadId;
    title.focus();
  });
  return (
    <>
      <Toolbar
        locale={locale}
        position={position}
        mailbox={mailbox}
        keyHints={keyHints}
        onClose={onClose}
      />
      <div className={styles.body} data-position={position}>
        {query.isPending && <ThreadSkeleton locale={locale} />}
        {query.isError && (
          <ErrorState
            message={m.mail_error({}, { locale })}
            retryLabel={m.retry_action({}, { locale })}
            onRetry={() => {
              void query.refetch();
            }}
          />
        )}
        {query.data === null && <EmptyState message={m.thread_gone({}, { locale })} />}
        {query.data !== undefined && query.data !== null && (
          <ThreadBody
            key={threadId}
            locale={locale}
            today={today}
            detail={query.data}
            position={position}
            titleRef={titleRef}
          />
        )}
      </div>
    </>
  );
}
