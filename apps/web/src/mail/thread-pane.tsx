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
import { Tooltip, TooltipGroup } from "../design-system/tooltip";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { PaneActions } from "./actions/pane-actions";
import { targetOfDetail } from "./actions/thread-target";
import { MessageCard } from "./message-card";
import { messagesOf, planMessages, subjectOf } from "./thread-messages";
import type { PlannedMessage } from "./thread-messages";
import { ThreadSkeleton } from "./thread-skeleton";
import { useInspect } from "./use-inspect";
import type { Inspect } from "./use-inspect";
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
  // Whether an action from a row opened the thread with the focus left
  // in the list, where the title then leaves it.
  listKeepsFocus: boolean;
  onClose: () => void;
}

interface ToolbarProps {
  locale: Locale;
  position: PanePosition;
  mailbox: Mailbox | undefined;
  // The thread as the pane holds it, whose state the flips follow.
  detail: ThreadDetail | null;
  keyHints: boolean;
  onClose: () => void;
}

// The thread's actions while it has a member in the mailbox shown.
function actionsOf(locale: Locale, mailbox: Mailbox | undefined, detail: ThreadDetail | null) {
  const thread =
    mailbox === undefined || detail === null ? null : targetOfDetail(detail, mailbox.id);
  return mailbox === undefined || thread === null ? null : { mailbox, thread, locale };
}

// The thread's actions head the pane, with Close at its end beside or
// below the list; on a screen of its own the way back to the mailbox
// comes first, the actions after it. Each button names its keys in a
// tooltip.
function Toolbar({ locale, position, mailbox, detail, keyHints, onClose }: ToolbarProps) {
  const actions = actionsOf(locale, mailbox, detail);
  if (position !== "screen") {
    const label = m.thread_close({}, { locale });
    return (
      <TooltipGroup>
        <div className={styles.toolbar}>
          {actions !== null && <PaneActions {...actions} />}
          <Tooltip label={label} keys={CLOSE_KEYS} align="end">
            <button
              type="button"
              className={cx(iconButton.button, styles.close)}
              aria-label={label}
              onClick={onClose}
            >
              <X className={iconButton.icon} aria-hidden="true" />
            </button>
          </Tooltip>
        </div>
      </TooltipGroup>
    );
  }
  const name = mailbox?.name ?? m.thread_back_unnamed({}, { locale });
  const label =
    mailbox === undefined
      ? m.thread_back_unnamed({}, { locale })
      : m.thread_back({ mailbox: mailbox.name }, { locale });
  return (
    <TooltipGroup>
      <div className={styles.toolbar} data-position="screen">
        <button type="button" className={styles.back} aria-label={label} onClick={onClose}>
          <ChevronLeft className={cx(iconButton.icon, styles.chevron)} aria-hidden="true" />
          <span className={styles.name}>{name}</span>
        </button>
        {/* The key stands outside the button, so the button shows nothing its name leaves out. */}
        {keyHints && <KeyCaps keys={CLOSE_KEYS} spoken={false} />}
        {actions !== null && <PaneActions {...actions} parted />}
      </div>
    </TooltipGroup>
  );
}

interface BodyProps {
  locale: Locale;
  today: number;
  cache: MailCache;
  accountId: string;
  detail: ThreadDetail;
  // Whether the detail is the thread as it stands; a cached one that
  // is being read again is not.
  current: boolean;
  position: PanePosition;
  titleRef: RefObject<HTMLHeadingElement | null>;
}

// Which cards stand open and which wait behind the button.
interface Drawn {
  open: ReadonlyMap<string, boolean>;
  older: ReadonlySet<string>;
}

function drawnOf(plan: readonly PlannedMessage[]): Drawn {
  return {
    open: new Map(plan.map((message) => [message.email.id, message.expanded])),
    older: new Set(plan.filter((message) => message.older).map((one) => one.email.id)),
  };
}

// The cards as the thread stood when it opened and as the user folded
// them since; null until the thread is read as it stands, so a thread
// opened again opens what arrived meanwhile. A message that lands
// later comes in folded, so it stays unread until its head opens it.
function useCardsDrawn(plan: readonly PlannedMessage[], current: boolean) {
  const [drawn, setDrawn] = useState(() => (current ? drawnOf(plan) : null));
  if (drawn === null && current) {
    setDrawn(drawnOf(plan));
  }
  const toggle = (id: string, open: boolean): void => {
    setDrawn((held) => held && { ...held, open: new Map(held.open).set(id, !open) });
  };
  return drawn === null ? null : { ...drawn, toggle };
}

interface CardsProps {
  locale: Locale;
  today: number;
  cache: MailCache;
  accountId: string;
  messages: readonly PlannedMessage[];
  open: ReadonlyMap<string, boolean>;
  inspectOf: (id: string) => Inspect;
  onToggle: (id: string, open: boolean) => void;
}

// One card per message; a message the map does not hold stands folded.
function Cards(props: CardsProps) {
  const { locale, today, cache, accountId, messages, open, inspectOf, onToggle } = props;
  return messages.map((message) => {
    const expanded = open.get(message.email.id) ?? false;
    return (
      <MessageCard
        key={message.email.id}
        locale={locale}
        today={today}
        message={message}
        expanded={expanded}
        cache={cache}
        accountId={accountId}
        inspect={inspectOf(message.email.id)}
        onToggle={() => {
          onToggle(message.email.id, expanded);
        }}
      />
    );
  });
}

// The subject, the count and one card per message, oldest first: as
// the thread opens the newest and the unread ones open, the rest
// collapsed, the oldest behind a button. A card toggles on its head;
// the inspect command stands in the palette while a card is open.
function ThreadBody(props: BodyProps) {
  const { locale, today, cache, accountId, detail, current, position, titleRef } = props;
  const messages = messagesOf(detail);
  const plan = planMessages(messages);
  const [olderShown, setOlderShown] = useState(false);
  const drawn = useCardsDrawn(plan, current);
  const cardsRef = useRef<HTMLOListElement>(null);
  const inspect = useInspect(
    cardsRef,
    locale,
    drawn !== null && [...drawn.open.values()].includes(true),
  );
  const revealedRef = useRef(false);
  // The button that revealed the older cards leaves; the first of them takes its focus.
  useLayoutEffect(() => {
    if (revealedRef.current) {
      revealedRef.current = false;
      cardsRef.current?.querySelector("button")?.focus();
    }
  });
  if (drawn === null) {
    return <ThreadSkeleton locale={locale} />;
  }
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
      {!olderShown && drawn.older.size > 0 && (
        <Button
          className={styles.older}
          onClick={() => {
            revealedRef.current = true;
            setOlderShown(true);
          }}
        >
          {m.thread_show_older({ count: drawn.older.size }, { locale })}
        </Button>
      )}
      {/* Safari drops the list semantics of a list without markers; the role keeps them. */}
      <ol ref={cardsRef} role="list" className={styles.cards}>
        <Cards
          locale={locale}
          today={today}
          cache={cache}
          accountId={accountId}
          messages={plan.filter((message) => olderShown || !drawn.older.has(message.email.id))}
          open={drawn.open}
          inspectOf={inspect.of}
          onToggle={drawn.toggle}
        />
      </ol>
    </div>
  );
}

// The title takes the focus once the thread is in, once per thread; a
// thread opened with the focus kept in the list leaves it there.
function useTitleFocus(
  titleRef: RefObject<HTMLHeadingElement | null>,
  threadId: string,
  kept: boolean,
): void {
  const focusedForRef = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (focusedForRef.current === threadId) {
      return;
    }
    if (kept) {
      focusedForRef.current = threadId;
      return;
    }
    const title = titleRef.current;
    if (title === null) {
      return;
    }
    focusedForRef.current = threadId;
    title.focus();
  });
}

// One open thread: its toolbar over the cards. Escape closes it from
// anywhere; the title takes the focus once the thread is in, unless an
// action from a row opened it and the focus stays in the list.
export function ThreadPane(props: ThreadPaneProps) {
  const { locale, cache, accountId, threadId, mailbox, position, keyHints, onClose } = props;
  const today = useToday();
  const query = useQuery(threadQueryOptions(cache, accountId, threadId));
  const titleRef = useRef<HTMLHeadingElement>(null);
  useTitleFocus(titleRef, threadId, props.listKeepsFocus);
  useCommand({
    id: "thread.close",
    label: m.thread_close({}, { locale }),
    group: "navigate",
    keys: CLOSE_KEYS,
    run: onClose,
  });
  return (
    <>
      <Toolbar
        locale={locale}
        position={position}
        mailbox={mailbox}
        detail={query.data ?? null}
        keyHints={keyHints}
        onClose={onClose}
      />
      <div className={styles.body} data-position={position}>
        {query.isPending && <ThreadSkeleton locale={locale} />}
        {/* A read that fails with an earlier answer held draws that answer alone. */}
        {query.isError && query.data === undefined && (
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
            cache={cache}
            accountId={accountId}
            detail={query.data}
            current={!(query.isStale && query.isFetching)}
            position={position}
            titleRef={titleRef}
          />
        )}
      </div>
    </>
  );
}
