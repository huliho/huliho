// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionMutations, moveMutations } from "@huliho/core";
import type {
  AfterAction,
  ListRow,
  MailAction,
  MailCache,
  Mailbox,
  Mutation,
  Undo,
} from "@huliho/core";
import { preferencesQueryOptions, threadQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { RefObject } from "react";

import { openMoveMode } from "../../commands/command-host";
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
import { ACTION_COMMANDS, BULK_KINDS, shownActions } from "./kinds";
import type { ActionKind } from "./kinds";
import { lendMove, moveLent, moveTargetsOf, rememberTarget, rememberedTarget } from "./move-mode";
import type { MoveSource } from "./move-mode";
import { lendOpenThreadRun } from "./open-thread";
import type { RowActionsHandle } from "./row-actions";
import { useSelectionCommands } from "./selection-commands";
import type { Selection } from "./selection";
import { offers, placesOf, takenBy } from "./taken";
import type { Aim, PlaceOf } from "./taken";
import { afterOf, aimAt, aimOf, survivorOf } from "./targets";
import { targetOfRow } from "./thread-target";
import { actionLabel, doneSentence } from "./words";

// The actions that take a thread out of the mailbox it acts from.
const LEAVING: ReadonlySet<MailAction> = new Set([
  "archive",
  "delete",
  "deleteForever",
  "spam",
  "notSpam",
  "move",
]);

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
  // The account's tree, whose roles and rights decide what each row offers.
  mailboxes: readonly Mailbox[];
  // The mailbox the open thread acts from where no page holds its row.
  openMailboxId: string | null;
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
  placeOf: PlaceOf;
  rule: AfterAction;
  // What an action takes, with the focus in the list or not.
  aimed: (inList: boolean) => Aimed;
  // The open thread alone, whatever the picks.
  open: () => Aim | null;
}

// The picks of the selection as an action takes them, each from the
// mailbox its row acts from.
function picksOf({ input, placeOf }: Context): Aim[] {
  return input.selection.picked().flatMap(({ row, index }) => {
    const place = placeOf(row.mailboxId);
    return place === null ? [] : [{ target: targetOfRow(row), index, place }];
  });
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

// Threads an action took out of their mailbox: they leave the picks and
// an open one among them makes way. A cursor whose row left keeps its
// place and so lands on the row after, unless rows above it left too;
// then it goes to the row that takes its place.
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

// The changes an action makes to one thread from the mailbox it acts
// from; a move goes to `to`, and nowhere without one.
function mutationsOf(action: MailAction, aim: Aim, to: string | null): Mutation[] {
  const { target, place } = aim;
  if (action !== "move") {
    return actionMutations(action, place.mailbox, place.offered, target);
  }
  return to === null ? [] : moveMutations(place.mailbox, place.offered, target, to);
}

// One thread an action changes, with the changes it makes to it.
interface Changed {
  aim: Aim;
  mutations: Mutation[];
}

// The threads an action changes among those it takes. One whose mailbox
// takes nothing of it, as a move to the mailbox it already stands in,
// is left out and stays where it is.
function changedOf(action: MailAction, taken: readonly Aim[], to: string | null): Changed[] {
  return taken.flatMap((aim) => {
    const mutations = mutationsOf(action, aim, to);
    return mutations.length === 0 ? [] : [{ aim, mutations }];
  });
}

// What an action did: the action, the changes it sent with the undo the
// worker answered and the mailbox a move went to.
interface Done {
  action: MailAction;
  mutations: readonly Mutation[];
  undo: Undo;
  to: string | null;
}

// A move says what it did in the toast and in the list's status region;
// a flip says nothing.
function sayDone(input: ActionsInput, { action, mutations, undo, to }: Done): void {
  const { cache, accountId, locale } = input;
  const entered = input.mailboxes.find((row) => row.id === to)?.name;
  const text = doneSentence(action, undo.threads.length, locale, entered);
  if (text !== null) {
    const emailIds = mutations.map(({ id }) => id);
    showActionToast({ cache, accountId, text, undo, emailIds });
    announce(input.regionRef.current, text);
  }
}

// One action on the threads a surface names, else on what the focus
// points at; a move goes to `to`. A change the log took nothing of
// leaves everything as it stands; one that took threads out of their
// mailbox says so, then settles the list. Resolves whether threads left
// their mailbox.
async function runAction(
  context: Context,
  kind: ActionKind,
  given?: Aimed,
  to: string | null = null,
): Promise<boolean> {
  const { input } = context;
  const inList = focusInList(input.scrollerRef.current, document.activeElement);
  const aimed = given ?? context.aimed(inList);
  const result = takenBy(kind, aimed.aims);
  const changed = result === null ? [] : changedOf(result.action, result.taken, to);
  if (result === null || changed.length === 0) {
    return false;
  }
  const { action } = result;
  const mutations = changed.flatMap((change) => change.mutations);
  const undo = await input.cache.mutateMany(input.accountId, mutations);
  if (aimed.picked) {
    input.selection.clear();
  }
  if (undo.threads.length === 0) {
    return false;
  }
  sayDone(input, { action, mutations, undo, to });
  const leaving = LEAVING.has(action);
  if (leaving) {
    const taken = changed.map((change) => change.aim);
    settleLeaving(context, taken, aimed.fromList ?? inList);
  }
  return leaving;
}

function launch(
  context: Context,
  kind: ActionKind,
  aimed?: Aimed,
  to: string | null = null,
): Promise<boolean> {
  return runAction(context, kind, aimed, to).catch((error: unknown) => {
    console.error("mail: an action failed", error instanceof Error ? error.message : String(error));
    return false;
  });
}

// What a move from `source` takes once its mailbox is picked; the focus
// decides for a key.
function aimedBy(context: Context, source: MoveSource): Aimed | undefined {
  if (source === "picks") {
    return { aims: picksOf(context), picked: true, fromList: true };
  }
  if (source === "open") {
    const aim = context.open();
    return { aims: aim === null ? [] : [aim], picked: false, fromList: false };
  }
  return undefined;
}

// The move picker for the threads `source` names, the mailboxes any of
// them may enter, the last one this device moved to from the account
// under Recent. The pick is remembered and moves through the list's
// latest render.
function pickMove(context: Context, source: MoveSource): void {
  const { accountId, mailboxes, locale, scrollerRef } = context.input;
  const aimed =
    aimedBy(context, source) ??
    context.aimed(focusInList(scrollerRef.current, document.activeElement));
  const entered = [...new Set(aimed.aims.flatMap(({ place }) => place.offered.move))];
  openMoveMode({
    targets: moveTargetsOf(entered, mailboxes, locale),
    recent: rememberedTarget(accountId),
    onPick: (to) => {
      rememberTarget(accountId, to);
      moveLent(to, source);
    },
  });
}

// What a surface's press of one kind does: a move opens the picker,
// every other action runs at once.
function press(context: Context, kind: ActionKind, source: MoveSource, aimed?: Aimed): void {
  if (kind === "move") {
    pickMove(context, source);
  } else {
    void launch(context, kind, aimed);
  }
}

// The command for one key while a thread stands to take it and the
// mailbox it acts from offers the action; its label follows the state
// of the threads the key takes from where the focus stands.
function commandOf(context: Context, shown: Aimed, kind: ActionKind): Command | null {
  const key = ACTION_COMMANDS.get(kind);
  const taken = takenBy(kind, shown.aims);
  if (key === undefined || taken === null || taken.taken.length === 0) {
    return null;
  }
  return {
    id: key.id,
    label: actionLabel(taken.action, context.input.locale),
    group: "act",
    keys: key.keys,
    run: () => {
      press(context, kind, "focus");
    },
  };
}

function kindOf(id: string): ActionKind | null {
  return [...ACTION_COMMANDS].find(([, command]) => command.id === id)?.[0] ?? null;
}

// Whether a mailbox a loaded row acts from offers an action picked
// threads could take.
function pickableIn(rows: ReadonlyMap<number, ListRow[]>, placeOf: PlaceOf): boolean {
  const ids = new Set([...rows.values()].flat().map((row) => row.mailboxId));
  return [...ids].some((id) => {
    const place = placeOf(id);
    return place !== null && BULK_KINDS.some((kind) => offers(place.offered, kind));
  });
}

// What the list hands its rows and the bulk bar.
export interface ListActions {
  // Whether a mailbox the rows act from offers an action picks could take.
  pickable: boolean;
  rows: RowActionsHandle;
  // Null while no selection stands.
  bulk: BulkHandle | null;
}

// The context of one render: the rule, the places and the aims.
function useActionContext(input: ActionsInput): Context {
  const { cache, accountId, rows, cursor, openThreadId, selection } = input;
  const rule = useQuery(preferencesQueryOptions).data?.afterAction ?? DEFAULT_AFTER_ACTION;
  // The open thread as the pane reads it, for one whose row no page holds.
  const opened = useQuery({
    ...threadQueryOptions(cache, accountId, openThreadId ?? ""),
    enabled: openThreadId !== null,
  });
  const detail = openThreadId === null ? null : (opened.data ?? null);
  const placeOf = placesOf(input.mailboxes);
  const where = { rows, cursor: cursor.active, openThreadId, detail, placeOf };
  const single = (inList: boolean): Aim | null =>
    aimOf({ ...where, inList, openMailboxId: input.openMailboxId });
  const context: Context = {
    input,
    placeOf,
    rule,
    aimed: (inList) => {
      if (selection.count > 0) {
        return { aims: picksOf(context), picked: true };
      }
      const aim = single(inList);
      return { aims: aim === null ? [] : [aim], picked: false };
    },
    open: () => (openThreadId === null ? null : single(false)),
  };
  return context;
}

// The thread actions as commands in the Act group, each row acting from
// the mailbox it shows, lent to the reading pane's toolbar for the open
// thread, handed to a row's buttons for their own thread and to the bulk
// bar for the picks. A key takes the picks while a selection stands, else
// the open thread unless the focus stands in the list, where it takes the
// cursor's row.
export function useActions(input: ActionsInput): ListActions {
  const context = useActionContext(input);
  const inGrid = useFocusInList(input.scrollerRef);
  // A pick in the move picker lands after the palette closed, so it acts
  // in the context of the latest render.
  useEffect(() =>
    lendMove((to, source) => {
      void launch(context, "move", aimedBy(context, source), to);
    }),
  );
  const shown = context.aimed(inGrid);
  useCommand(commandOf(context, shown, "archive"));
  useCommand(commandOf(context, shown, "delete"));
  useCommand(commandOf(context, shown, "spam"));
  useCommand(commandOf(context, shown, "read"));
  useCommand(commandOf(context, shown, "star"));
  useCommand(commandOf(context, shown, "move"));
  const pickable = pickableIn(input.rows, context.placeOf);
  useSelectionCommands({ ...input, pickable });
  // A press in the toolbar acts from the pane, so the next thread's title
  // takes the focus whatever held it. Each render lends its own context.
  useEffect(() =>
    lendOpenThreadRun((id) => {
      const kind = kindOf(id);
      const aim = context.open();
      if (kind !== null && aim !== null) {
        press(context, kind, "open", { aims: [aim], picked: false, fromList: false });
      }
    }),
  );
  return {
    pickable,
    rows: {
      placeOf: context.placeOf,
      run: (kind, row, index) => {
        const aim = aimAt(row, index, context.placeOf);
        return launch(context, kind, { aims: aim === null ? [] : [aim], picked: false });
      },
    },
    bulk: bulkOf(context, shown),
  };
}

// The bar's handle while a selection stands: the actions the picks can
// take, a press there acting on them as one from the list.
function bulkOf(context: Context, shown: Aimed): BulkHandle | null {
  const { selection, locale } = context.input;
  if (!selection.open) {
    return null;
  }
  return {
    count: selection.count,
    shown: shownActions(BULK_KINDS, shown.picked ? shown.aims : [], locale),
    run: (kind) => {
      press(context, kind, "picks", { aims: picksOf(context), picked: true, fromList: true });
    },
    close: selection.clear,
  };
}
