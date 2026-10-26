// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, JobProgress, MailCache, Mailbox } from "@huliho/core";
import { Search } from "lucide-react";
import {
  Suspense,
  use,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { FocusEvent, ReactElement, RefObject } from "react";

import { commandsSnapshot, subscribeCommands } from "../commands/registry";
import { Avatar } from "../design-system/avatar";
import { cx } from "../design-system/cx";
import iconButton from "../design-system/icon-button.module.css";
import { Skeleton } from "../design-system/skeleton";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useCoarsePointer } from "../shell/pointer";
import { jobItems, useFolderCommands } from "./actions/folder-jobs";
import type { FolderItem, FolderJobInput } from "./actions/folder-jobs";
import { JobProgressBar, OfflineAsk } from "./actions/job-progress";
import { onJobEnd, useAskedOffline, useRunningJob } from "./actions/job-store";
import { endSentence, runningSentence } from "./actions/job-words";
import { startSelection } from "./actions/selection-lend";
import type { FolderMenuProps } from "./folder-menu";
import { focusLost } from "./list-cursor";
import { prefetchMenus } from "./menus-chunk";
import { MonoCount, countWords } from "./mono-count";
import { SEARCH_KEYS, SEARCH_MARK, takesReturn } from "./search/open-search";
import { useStatusText } from "./status-text";
import { countOf } from "./tree";
import styles from "./pane-header.module.css";

interface PaneHeaderProps {
  locale: Locale;
  cache: MailCache;
  account: AccountRow;
  // The open mailbox; undefined while the tree loads or resolves the inbox.
  mailbox: Mailbox | undefined;
  // The account's tree, which names the mailbox a job runs on.
  mailboxes: readonly Mailbox[];
  pending: boolean;
  // Present at the phone width, where the avatar opens the sidebar.
  onOpenSidebar?: (() => void) | undefined;
}

function LoadedFolderMenu(props: FolderMenuProps) {
  const { FolderMenu } = use(prefetchMenus());
  return <FolderMenu {...props} />;
}

function pickableNow(): boolean {
  return commandsSnapshot().some((command) => command.id === "selection.toggle");
}

// Whether the list lets threads be picked, as its key in the registry
// says; the header renders again only when that flips.
function usePickable(): boolean {
  return useSyncExternalStore(subscribeCommands, pickableNow, pickableNow);
}

// The items of the mailbox's menu: Select on touch while the list lets
// threads be picked, then the jobs the mailbox offers.
function menuItems(input: FolderJobInput, running: boolean, select: boolean): FolderItem[] {
  const first: FolderItem[] = select
    ? [
        {
          id: "select",
          label: m.command_selection({ action: "select" }, { locale: input.locale }),
          danger: false,
          run: startSelection,
        },
      ]
    : [];
  return [...first, ...jobItems(input, running)];
}

interface TitleProps {
  mailbox: Mailbox;
  items: readonly FolderItem[];
}

// The mailbox's name, the button of its menu while the menu holds an item.
function Title({ mailbox, items }: TitleProps) {
  return (
    <h1 className={styles.title}>
      {items.length > 0 ? (
        <Suspense fallback={mailbox.name}>
          <LoadedFolderMenu name={mailbox.name} items={items} />
        </Suspense>
      ) : (
        mailbox.name
      )}
    </h1>
  );
}

function HintedButton({ label, button }: { label: string; button: ReactElement }) {
  const { Tooltip } = use(prefetchMenus());
  return (
    <Tooltip label={label} keys={SEARCH_KEYS} align="end">
      {button}
    </Tooltip>
  );
}

// The search's button at the header's end, running the command `/` runs;
// its hint rides the menus chunk. A search closed from it hands the focus
// back here.
function SearchButton({ locale }: { locale: Locale }) {
  const ref = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    if (takesReturn("button")) {
      ref.current?.focus({ preventScroll: true });
    }
  }, []);
  const label = m.command_search({}, { locale });
  const button = (
    <button
      ref={ref}
      type="button"
      className={cx(iconButton.button, styles.search)}
      aria-label={label}
      aria-keyshortcuts="/"
      {...SEARCH_MARK}
      onClick={(event) => {
        // Safari leaves a pressed button without the focus; the search
        // hands the focus back to where it stood.
        event.currentTarget.focus({ preventScroll: true });
        commandsSnapshot()
          .findLast((command) => command.id === "search.open")
          ?.run();
      }}
    >
      <Search className={iconButton.icon} aria-hidden="true" />
    </button>
  );
  return (
    <Suspense fallback={button}>
      <HintedButton label={label} button={button} />
    </Suspense>
  );
}

// Every job of the mailbox shown reaches the palette; a child of its own,
// so registering never renders the header again.
function FolderCommands({ items }: { items: readonly FolderItem[] }) {
  useFolderCommands(items);
  return null;
}

// What the header's status region says: the running job's sentence, the
// offline answer, else how the account's last job ended.
function useHeaderSentence(
  locale: Locale,
  account: AccountRow,
  mailboxes: readonly Mailbox[],
  running: Pick<SlotProps, "job" | "offline">,
): string {
  const [ended, setEnded] = useState("");
  useEffect(
    () =>
      onJobEnd((accountId, progress) => {
        const box = mailboxes.find((row) => row.id === progress.job.mailboxId);
        if (accountId === account.id && box !== undefined) {
          setEnded(endSentence(progress, box, locale));
        }
      }),
    [account.id, mailboxes, locale],
  );
  if (running.job !== null) {
    return runningSentence(running.job.progress, running.job.box, locale);
  }
  return running.offline ? m.job_offline({}, { locale }) : ended;
}

// Where the focus goes once the header swapped away the control that
// held it: the control in its place, else the list below.
function nextFocus(header: HTMLElement): HTMLElement | null {
  const list = header.parentElement;
  return (
    header.querySelector<HTMLElement>("h1 button, [data-progress] button") ??
    list?.querySelector<HTMLElement>('[role="row"][tabindex="0"]') ??
    list?.querySelector<HTMLElement>('[tabindex="-1"]') ??
    null
  );
}

// A control the header swaps away while it holds the focus hands it on.
// The focus is followed through its events, since a removed element
// sends none.
function useFocusKept(headerRef: RefObject<HTMLElement | null>, mode: string) {
  const heldRef = useRef(false);
  const modeRef = useRef(mode);
  useLayoutEffect(() => {
    const header = headerRef.current;
    if (header !== null && modeRef.current !== mode && heldRef.current && focusLost()) {
      heldRef.current = false;
      nextFocus(header)?.focus({ preventScroll: true });
    }
    modeRef.current = mode;
  });
  return {
    onFocus: () => {
      heldRef.current = true;
    },
    onBlur: (event: FocusEvent<HTMLElement>) => {
      heldRef.current = event.currentTarget.contains(event.relatedTarget);
    },
  };
}

interface SlotProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox | undefined;
  // The running job and the mailbox it runs on, where this tree has it.
  job: { progress: JobProgress; box: Mailbox } | null;
  offline: boolean;
  items: readonly FolderItem[];
}

// What the header holds after the sidebar's button: a running job, else
// the answer to a job asked offline, else the mailbox's name and count.
function Slot({ locale, cache, accountId, mailbox, job, offline, items }: SlotProps) {
  if (job !== null) {
    return (
      <JobProgressBar
        locale={locale}
        cache={cache}
        accountId={accountId}
        progress={job.progress}
        mailbox={job.box}
      />
    );
  }
  if (offline) {
    return <OfflineAsk locale={locale} />;
  }
  if (mailbox === undefined) {
    return null;
  }
  const count = countOf(mailbox);
  return (
    <>
      <Title mailbox={mailbox} items={items} />
      {count > 0 && (
        <MonoCount
          value={count}
          locale={locale}
          tone="accent"
          label={countWords(mailbox, locale)}
        />
      )}
    </>
  );
}

// The shape of the slot, as the hand-off tells one from the next: the
// name is a button while its menu holds an item, else plain text.
function modeOf(job: SlotProps["job"], offline: boolean, titled: boolean): string {
  if (job !== null) {
    return "job";
  }
  if (offline) {
    return "offline";
  }
  return titled ? "menu" : "name";
}

// The bar above the list: the mailbox's name and unread count, the
// count worded for a reader who cannot see what it stands beside; a
// running folder job, or the answer to one asked offline, in their place.
export function PaneHeader(props: PaneHeaderProps) {
  const { locale, cache, account, mailbox, mailboxes, pending, onOpenSidebar } = props;
  const progress = useRunningJob(account.id);
  const offline = useAskedOffline(account.id);
  const touch = useCoarsePointer();
  const pickable = usePickable();
  const box = mailboxes.find((row) => row.id === progress?.job.mailboxId);
  const job = progress === null || box === undefined ? null : { progress, box };
  const headerRef = useRef<HTMLElement>(null);
  const regionRef = useRef<HTMLSpanElement>(null);
  useStatusText(regionRef, useHeaderSentence(locale, account, mailboxes, { job, offline }));
  const items =
    mailbox === undefined
      ? []
      : menuItems(
          { locale, cache, accountId: account.id, mailbox },
          progress !== null,
          touch && pickable,
        );
  const kept = useFocusKept(headerRef, modeOf(job, offline, items.length > 0));
  return (
    <header ref={headerRef} className={styles.header} onFocus={kept.onFocus} onBlur={kept.onBlur}>
      {onOpenSidebar !== undefined && (
        <button
          type="button"
          className={styles.sidebarButton}
          aria-label={m.mail_sidebar({}, { locale })}
          onClick={onOpenSidebar}
        >
          <Avatar name={account.name} locale={locale} />
        </button>
      )}
      <Slot
        locale={locale}
        cache={cache}
        accountId={account.id}
        mailbox={mailbox}
        job={job}
        offline={offline}
        items={items}
      />
      {mailbox === undefined && pending && <Skeleton className={styles.titleSkeleton} />}
      <SearchButton locale={locale} />
      <FolderCommands items={items} />
      <span ref={regionRef} role="status" className={spoken.spoken} />
    </header>
  );
}
