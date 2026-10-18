// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionFor, actionMutations, offeredActions } from "@huliho/core";
import type { AfterAction, ListRow, MailAction, MailCache, Mailbox, Offered } from "@huliho/core";
import { preferencesQueryOptions, threadQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { RefObject } from "react";

import { CLAIMED_BY_LAYER } from "../../commands/registry";
import type { Command } from "../../commands/registry";
import { useCommand } from "../../commands/use-command";
import type { Locale } from "../../paraglide/runtime.js";
import { DEFAULT_AFTER_ACTION } from "../../theme/appearance";
import type { ListCursor } from "../list-cursor";
import type { OpenFocus } from "../thread-history";
import { showActionToast } from "./action-toast";
import { ACTION_COMMANDS } from "./kinds";
import type { ActionKind } from "./kinds";
import { lendOpenThreadRun } from "./open-thread";
import type { RowActionsHandle } from "./row-actions";
import { afterOf, aimOf } from "./targets";
import type { Aim } from "./targets";
import { targetOfRow } from "./thread-target";
import { actionLabel, doneSentence } from "./words";

// The actions that take a thread out of the mailbox shown.
const LEAVING: ReadonlySet<MailAction> = new Set([
  "archive",
  "delete",
  "deleteForever",
  "spam",
  "notSpam",
]);

const OFFERS: ReadonlyMap<ActionKind, (offered: Offered) => boolean> = new Map([
  ["archive", (offered) => offered.archive !== null],
  ["delete", (offered) => offered.delete !== null || offered.deleteForever],
  ["spam", (offered) => offered.spam !== null],
  ["read", (offered) => offered.seen],
  ["star", (offered) => offered.flag],
]);

function offers(offered: Offered, kind: ActionKind): boolean {
  return OFFERS.get(kind)?.(offered) === true;
}

// Whether `focused` stands in the list's grid. A grid under an inert
// ancestor, as the list under a thread screen, cannot hold the focus,
// so a row focused before the screen opened counts as nobody's focus.
function focusInList(scroller: HTMLElement | null, focused: EventTarget | null): boolean {
  return (
    scroller !== null &&
    focused instanceof Node &&
    scroller.contains(focused) &&
    scroller.closest("[inert]") === null
  );
}

// Whether the focus stands in the list's grid, followed through the
// document's focus events so the labels name what a key does from
// there. A focus inside a layered surface counts where it stood, since
// the surface gives it back before a pick runs.
function useFocusInList(scrollerRef: RefObject<HTMLElement | null>): boolean {
  const [inList, setInList] = useState(false);
  useEffect(() => {
    const read = (event: FocusEvent): void => {
      const next = event.type === "focusin" ? event.target : event.relatedTarget;
      if (next instanceof Element && next.closest(CLAIMED_BY_LAYER) !== null) {
        return;
      }
      setInList(focusInList(scrollerRef.current, next));
    };
    document.addEventListener("focusin", read);
    document.addEventListener("focusout", read);
    return () => {
      document.removeEventListener("focusin", read);
      document.removeEventListener("focusout", read);
    };
  }, [scrollerRef]);
  return inList;
}

// A sentence for the list's status region, so a reader whose focus
// moved hears it too. The region empties first, so the same sentence
// twice is said twice.
function announce(region: HTMLElement | null, text: string): void {
  if (region === null) {
    return;
  }
  region.textContent = "";
  requestAnimationFrame(() => {
    region.textContent = text;
  });
}

export interface ActionsInput {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
  mailboxes: readonly Mailbox[];
  rows: ReadonlyMap<number, ListRow[]>;
  cursor: ListCursor;
  openThreadId: string | null;
  scrollerRef: RefObject<HTMLElement | null>;
  regionRef: RefObject<HTMLElement | null>;
  onOpen: (row: ListRow, focus: OpenFocus) => void;
  onClose: () => void;
}

interface Context {
  input: ActionsInput;
  offered: Offered;
  rule: AfterAction;
  // The thread an action takes, with the focus in the list or not.
  aim: (inList: boolean) => Aim | null;
}

// The open thread an action took out of the list makes way: the next
// one opens or the thread closes, with the cursor where the rule puts
// it. The focus follows where the action came from: an action from the
// list leaves it on the cursor's row, one from the pane hands it to the
// next thread's title.
function makeWay(context: Context, aim: Aim, fromList: boolean): void {
  const { input } = context;
  const after = afterOf(input.rows, aim.index, context.rule);
  if (after.index !== null) {
    input.cursor.place(after.index);
  }
  if ("open" in after) {
    input.onOpen(after.open, fromList ? "list" : "title");
  } else {
    input.onClose();
  }
}

// A thread a surface names for an action, and whether the action counts
// as one from the list; the focus decides where the surface leaves it.
interface Aimed {
  aim: Aim;
  fromList?: boolean;
}

// One action on a thread: the one a surface names, else the one the
// focus points at. A change the log took nothing of leaves everything
// as it stands; one that took threads out of the mailbox shown says
// so in the toast and the status region, then an open thread among
// them makes way.
async function runAction(context: Context, kind: ActionKind, aimed?: Aimed): Promise<void> {
  const { input } = context;
  const { cache, accountId, mailbox, locale } = input;
  const inList = focusInList(input.scrollerRef.current, document.activeElement);
  const aim = aimed === undefined ? context.aim(inList) : aimed.aim;
  if (aim === null) {
    return;
  }
  const action = actionFor(kind, mailbox, aim.target);
  const mutations = actionMutations(action, mailbox, context.offered, aim.target);
  if (mutations.length === 0) {
    return;
  }
  const undo = await cache.mutateMany(accountId, mutations);
  if (undo.threads.length === 0) {
    return;
  }
  const text = doneSentence(action, undo.threads.length, locale);
  if (text !== null) {
    const emailIds = mutations.map(({ id }) => id);
    showActionToast({ cache, accountId, text, undo, emailIds });
    announce(input.regionRef.current, text);
  }
  if (LEAVING.has(action) && aim.target.threadId === input.openThreadId) {
    makeWay(context, aim, aimed?.fromList ?? inList);
  }
}

function launch(context: Context, kind: ActionKind, aimed?: Aimed): void {
  runAction(context, kind, aimed).catch((error: unknown) => {
    console.error("mail: an action failed", error instanceof Error ? error.message : String(error));
  });
}

// The command for one key while the mailbox offers its action and a
// thread stands to take it; its label follows the state of the thread
// the key takes from where the focus stands.
function commandOf(context: Context, shown: Aim | null, kind: ActionKind): Command | null {
  const key = ACTION_COMMANDS.get(kind);
  const { mailbox, locale } = context.input;
  if (shown === null || key === undefined || !offers(context.offered, kind)) {
    return null;
  }
  return {
    id: key.id,
    label: actionLabel(actionFor(kind, mailbox, shown.target), locale),
    group: "act",
    keys: key.keys,
    run: () => {
      launch(context, kind);
    },
  };
}

function kindOf(id: string): ActionKind | null {
  return [...ACTION_COMMANDS].find(([, command]) => command.id === id)?.[0] ?? null;
}

// The thread actions of a mailbox as commands in the Act group, lent to
// the reading pane's toolbar for the open thread and to a row's buttons
// for their own thread. A key takes the open thread unless the focus
// stands in the list, where it takes the cursor's row.
export function useActions(input: ActionsInput): RowActionsHandle {
  const { cache, accountId, mailbox, mailboxes, rows, cursor, openThreadId } = input;
  const rule = useQuery(preferencesQueryOptions).data?.afterAction ?? DEFAULT_AFTER_ACTION;
  const inGrid = useFocusInList(input.scrollerRef);
  // The open thread as the pane reads it, for one whose row no page holds.
  const opened = useQuery({
    ...threadQueryOptions(cache, accountId, openThreadId ?? ""),
    enabled: openThreadId !== null,
  });
  const detail = openThreadId === null ? null : (opened.data ?? null);
  const context: Context = {
    input,
    offered: offeredActions(mailbox, mailboxes),
    rule,
    aim: (inList) =>
      aimOf({ rows, cursor: cursor.active, openThreadId, inList, detail, mailboxId: mailbox.id }),
  };
  const shown = context.aim(inGrid);
  useCommand(commandOf(context, shown, "archive"));
  useCommand(commandOf(context, shown, "delete"));
  useCommand(commandOf(context, shown, "spam"));
  useCommand(commandOf(context, shown, "read"));
  useCommand(commandOf(context, shown, "star"));
  // A press in the toolbar acts from the pane, so the next thread's title
  // takes the focus whatever held it. Each render lends its own context.
  useEffect(() =>
    lendOpenThreadRun((id) => {
      const kind = kindOf(id);
      const aim = openThreadId === null ? null : context.aim(false);
      if (kind !== null && aim !== null) {
        launch(context, kind, { aim, fromList: false });
      }
    }),
  );
  return {
    mailbox,
    run: (kind, row, index) => {
      launch(context, kind, { aim: { target: targetOfRow(row), index } });
    },
  };
}
