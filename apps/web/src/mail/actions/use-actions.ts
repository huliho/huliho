// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionMutations, offeredActions } from "@huliho/core";
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
import { announce } from "./announce";
import type { BulkHandle } from "./bulk-handle";
import { ACTION_COMMANDS, BULK_KINDS } from "./kinds";
import type { ActionKind } from "./kinds";
import { lendOpenThreadRun } from "./open-thread";
import type { RowActionsHandle } from "./row-actions";
import { useSelectionCommands } from "./selection-commands";
import type { Selection } from "./selection";
import { afterOf, aimOf, combinedOf, survivorOf, takenBy } from "./targets";
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

export interface ActionsInput {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
  mailboxes: readonly Mailbox[];
  rows: ReadonlyMap<number, ListRow[]>;
  cursor: ListCursor;
  selection: Selection;
  openThreadId: string | null;
  scrollerRef: RefObject<HTMLElement | null>;
  regionRef: RefObject<HTMLElement | null>;
  onOpen: (row: ListRow, focus: OpenFocus) => void;
  onClose: () => void;
}

// The threads an action takes and where it came from: the picks of a
// standing selection, else one thread a surface or the focus names.
interface Aimed {
  aims: Aim[];
  // Whether the aims are the picks, which the action then lets go of.
  picked: boolean;
  // Whether the action counts as one from the list; the focus decides
  // where a surface leaves it unsaid.
  fromList?: boolean;
}

interface Context {
  input: ActionsInput;
  offered: Offered;
  rule: AfterAction;
  // What an action takes, with the focus in the list or not.
  aimed: (inList: boolean) => Aimed;
  // The open thread alone, whatever the picks.
  open: () => Aim | null;
}

// The picks of the selection as an action takes them.
function picksOf(selection: Selection): Aim[] {
  return selection.picked().map(({ row, index }) => ({ target: targetOfRow(row), index }));
}

// The open thread an action took out of the list makes way: the next
// one that stays opens or the thread closes, with the cursor where the
// rule puts it. The focus follows where the action came from: an action
// from the list leaves it on the cursor's row, one from the pane hands
// it to the next thread's title.
function makeWay(context: Context, index: number | null, leaving: Set<number>, fromList: boolean) {
  const { input } = context;
  const after = afterOf(input.rows, index, context.rule, leaving);
  if (after.index !== null) {
    input.cursor.place(after.index);
  }
  if ("open" in after) {
    input.onOpen(after.open, fromList ? "list" : "title");
  } else {
    input.onClose();
  }
}

// Threads an action took out of the list: they leave the picks and an
// open one among them makes way. A cursor whose row left keeps its place
// and so lands on the row after, unless rows above it left too; then it
// goes to the row that takes its place.
function settleLeaving(context: Context, taken: readonly Aim[], fromList: boolean): void {
  const { input } = context;
  const { active } = input.cursor;
  const leaving = new Set(taken.flatMap(({ index }) => (index === null ? [] : [index])));
  input.selection.drop(taken.map(({ target }) => target.threadId));
  const open = taken.find(({ target }) => target.threadId === input.openThreadId);
  if (open !== undefined) {
    makeWay(context, open.index, leaving, fromList);
    return;
  }
  const shifted = leaving.has(active) && [...leaving].some((index) => index < active);
  const survivor = shifted ? survivorOf(input.rows, active, leaving) : null;
  if (survivor !== null) {
    input.cursor.place(survivor.index);
  }
}

// One action on the threads a surface names, else on what the focus
// points at. A change the log took nothing of leaves everything as it
// stands; one that took threads out of the mailbox shown says so in the
// toast and the status region, then settles the list.
async function runAction(context: Context, kind: ActionKind, given?: Aimed): Promise<void> {
  const { input } = context;
  const { cache, accountId, mailbox, locale } = input;
  const inList = focusInList(input.scrollerRef.current, document.activeElement);
  const aimed = given ?? context.aimed(inList);
  const { action, taken } = takenBy(kind, mailbox, aimed.aims);
  const mutations = taken.flatMap(({ target }) =>
    actionMutations(action, mailbox, context.offered, target),
  );
  if (mutations.length === 0) {
    return;
  }
  const undo = await cache.mutateMany(accountId, mutations);
  if (aimed.picked) {
    input.selection.clear();
  }
  if (undo.threads.length === 0) {
    return;
  }
  const text = doneSentence(action, undo.threads.length, locale);
  if (text !== null) {
    const emailIds = mutations.map(({ id }) => id);
    showActionToast({ cache, accountId, text, undo, emailIds });
    announce(input.regionRef.current, text);
  }
  if (LEAVING.has(action)) {
    settleLeaving(context, taken, aimed.fromList ?? inList);
  }
}

function launch(context: Context, kind: ActionKind, aimed?: Aimed): void {
  runAction(context, kind, aimed).catch((error: unknown) => {
    console.error("mail: an action failed", error instanceof Error ? error.message : String(error));
  });
}

// The command for one key while the mailbox offers its action and a
// thread stands to take it; its label follows the state of the threads
// the key takes from where the focus stands.
function commandOf(context: Context, shown: Aimed, kind: ActionKind): Command | null {
  const key = ACTION_COMMANDS.get(kind);
  const { mailbox, locale } = context.input;
  if (shown.aims.length === 0 || key === undefined || !offers(context.offered, kind)) {
    return null;
  }
  return {
    id: key.id,
    label: actionLabel(takenBy(kind, mailbox, shown.aims).action, locale),
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

// What the list hands its rows and the bulk bar.
export interface ListActions {
  // Whether the mailbox offers an action picked threads could take.
  pickable: boolean;
  rows: RowActionsHandle;
  // Null while no selection stands.
  bulk: BulkHandle | null;
}

// The thread actions of a mailbox as commands in the Act group, lent to
// the reading pane's toolbar for the open thread, handed to a row's
// buttons for their own thread and to the bulk bar for the picks. A key
// takes the picks while a selection stands, else the open thread unless
// the focus stands in the list, where it takes the cursor's row.
export function useActions(input: ActionsInput): ListActions {
  const { cache, accountId, mailbox, mailboxes, rows, cursor, openThreadId, selection } = input;
  const rule = useQuery(preferencesQueryOptions).data?.afterAction ?? DEFAULT_AFTER_ACTION;
  const inGrid = useFocusInList(input.scrollerRef);
  // The open thread as the pane reads it, for one whose row no page holds.
  const opened = useQuery({
    ...threadQueryOptions(cache, accountId, openThreadId ?? ""),
    enabled: openThreadId !== null,
  });
  const detail = openThreadId === null ? null : (opened.data ?? null);
  const single = (inList: boolean): Aim | null =>
    aimOf({ rows, cursor: cursor.active, openThreadId, inList, detail, mailboxId: mailbox.id });
  const context: Context = {
    input,
    offered: offeredActions(mailbox, mailboxes),
    rule,
    aimed: (inList) => {
      if (selection.count > 0) {
        return { aims: picksOf(selection), picked: true };
      }
      const aim = single(inList);
      return { aims: aim === null ? [] : [aim], picked: false };
    },
    open: () => (openThreadId === null ? null : single(false)),
  };
  const shown = context.aimed(inGrid);
  useCommand(commandOf(context, shown, "archive"));
  useCommand(commandOf(context, shown, "delete"));
  useCommand(commandOf(context, shown, "spam"));
  useCommand(commandOf(context, shown, "read"));
  useCommand(commandOf(context, shown, "star"));
  const pickable = BULK_KINDS.some((kind) => offers(context.offered, kind));
  useSelectionCommands({ ...input, pickable });
  // A press in the toolbar acts from the pane, so the next thread's title
  // takes the focus whatever held it. Each render lends its own context.
  useEffect(() =>
    lendOpenThreadRun((id) => {
      const kind = kindOf(id);
      const aim = context.open();
      if (kind !== null && aim !== null) {
        launch(context, kind, { aims: [aim], picked: false, fromList: false });
      }
    }),
  );
  return {
    pickable,
    rows: {
      mailbox,
      run: (kind, row, index) => {
        launch(context, kind, { aims: [{ target: targetOfRow(row), index }], picked: false });
      },
    },
    bulk: bulkOf(context, shown),
  };
}

// The bar's handle while a selection stands: a press there acts on the
// picks as one from the list.
function bulkOf(context: Context, shown: Aimed): BulkHandle | null {
  const { mailbox, selection } = context.input;
  if (!selection.open) {
    return null;
  }
  return {
    mailbox,
    count: selection.count,
    thread: combinedOf(shown.picked ? shown.aims : []),
    run: (kind) => {
      launch(context, kind, { aims: picksOf(selection), picked: true, fromList: true });
    },
    close: selection.clear,
  };
}
