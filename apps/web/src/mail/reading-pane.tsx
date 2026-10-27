// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache } from "@huliho/core";
import { useLocation, useNavigate } from "@tanstack/react-router";
import { Suspense, use, useEffect } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { chunk } from "../shell/chunk";
import type { Place } from "./actions/taken";
import { unmarkedForList, wantsListFocus } from "./thread-history";
import type { PanePosition, ThreadPaneProps, WayBack } from "./thread-pane";
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

// The thread the address names: the mailbox it acts from, where the way
// back leads and the address's path.
export interface OpenThread {
  accountId: string;
  threadId: string;
  place: Place | null;
  back: WayBack;
  path: string;
}

// Whether an action from a row opened the thread with the focus left in
// the list. The mark counts once the address names the thread shown,
// since the address moves on before the route's params do. The entry
// then loses its mark, so a later visit to it focuses the title as any
// open does.
function useListKeepsFocus({ path }: OpenThread): boolean {
  const navigate = useNavigate();
  const marked = useLocation({
    select: (location) => (wantsListFocus(location.state) ? location.pathname : null),
  });
  const kept = marked === path;
  useEffect(() => {
    if (kept) {
      void navigate({ to: ".", search: (held) => held, replace: true, state: unmarkedForList });
    }
  }, [kept, navigate]);
  return kept;
}

interface OpenedProps {
  locale: Locale;
  cache: MailCache;
  thread: OpenThread;
  position: PanePosition;
  keyHints: boolean;
  onClose: () => void;
}

// The pane of the open thread, told whether the focus stays in the list.
function Opened({ thread, ...props }: OpenedProps) {
  const listKeepsFocus = useListKeepsFocus(thread);
  return (
    <LoadedThreadPane
      {...props}
      accountId={thread.accountId}
      threadId={thread.threadId}
      place={thread.place}
      back={thread.back}
      listKeepsFocus={listKeepsFocus}
    />
  );
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
          <Opened
            locale={locale}
            cache={cache}
            thread={thread}
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
        <Opened
          locale={locale}
          cache={cache}
          thread={thread}
          position="screen"
          keyHints={keyHints}
          onClose={onClose}
        />
      </Suspense>
    </section>
  );
}
