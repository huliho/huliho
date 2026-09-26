// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, Mailbox } from "@huliho/core";
import { Suspense, use } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { chunk } from "../shell/chunk";
import type { PanePosition, ThreadPaneProps } from "./thread-pane";
import { ThreadSkeleton } from "./thread-skeleton";
import styles from "./reading-pane.module.css";

// The pane's code is a chunk of its own, outside the initial bundle.
// The shell fetches it on mount; once it is in, a render reads it in
// the same pass, so the first open shows no still cards for it.
export const prefetchThreadPane = chunk(() => import("./thread-pane"));

function LoadedThreadPane(props: ThreadPaneProps) {
  const { ThreadPane } = use(prefetchThreadPane());
  return <ThreadPane {...props} />;
}

// The thread the address names, with the mailbox it was opened from.
export interface OpenThread {
  accountId: string;
  threadId: string;
  mailbox: Mailbox | undefined;
}

interface ReadingPaneProps {
  locale: Locale;
  cache: MailCache;
  thread: OpenThread | null;
  position: PanePosition;
  keyHints: boolean;
  onClose: () => void;
}

// The third pane beside or below the list: the open thread, or a word
// while nothing is open.
export function ReadingPane(props: ReadingPaneProps) {
  const { locale, cache, thread, position, keyHints, onClose } = props;
  return (
    <aside
      className={styles.pane}
      data-empty={thread === null || undefined}
      aria-label={m.thread_pane({}, { locale })}
    >
      {thread === null ? (
        <p className={styles.nothingOpen}>{m.thread_none_open({}, { locale })}</p>
      ) : (
        <Suspense fallback={<ThreadSkeleton locale={locale} />}>
          <LoadedThreadPane
            locale={locale}
            cache={cache}
            accountId={thread.accountId}
            threadId={thread.threadId}
            mailbox={thread.mailbox}
            position={position}
            keyHints={keyHints}
            onClose={onClose}
          />
        </Suspense>
      )}
    </aside>
  );
}

interface ThreadScreenProps {
  locale: Locale;
  cache: MailCache;
  thread: OpenThread;
  keyHints: boolean;
  onClose: () => void;
}

// The thread as a screen of its own, over the list: a phone always
// opens it so, and the pane switched off does at every width.
export function ThreadScreen({ locale, cache, thread, keyHints, onClose }: ThreadScreenProps) {
  return (
    <section className={styles.screen} aria-label={m.thread_pane({}, { locale })}>
      <Suspense fallback={<ThreadSkeleton locale={locale} />}>
        <LoadedThreadPane
          locale={locale}
          cache={cache}
          accountId={thread.accountId}
          threadId={thread.threadId}
          mailbox={thread.mailbox}
          position="screen"
          keyHints={keyHints}
          onClose={onClose}
        />
      </Suspense>
    </section>
  );
}
