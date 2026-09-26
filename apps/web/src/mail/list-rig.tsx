// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  JmapClient,
  JmapError,
  MemoryMailStore,
  applyChanges,
  listPage,
  queryWindow,
  revealNewMail,
} from "@huliho/core";
import type { ListPage, ListRow, MailCache, Mailbox } from "@huliho/core";
import { ACCOUNT, FakeJmap, at, email, mailbox as mailboxRow } from "@huliho/core/testing";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { JSXElementConstructor, ReactNode } from "react";
import { afterEach, beforeEach, vi } from "vitest";

import { dispatchKey } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import {
  FASTMAIL,
  FIXED_NOW,
  INBOX_ID,
  INBOX_PAGE,
  MAILBOXES,
  fixtureCache,
  pageOf,
} from "./fixtures";
import type { Draft, PageAnswer } from "./fixtures";
import { startOfDay } from "./row-time";
import { ThreadList } from "./thread-list";

// The rig the thread list tests render in: a scroll box of ten rows of
// the comfortable height, a cache of the caller's choice and the keys.
const TODAY = startOfDay(FIXED_NOW);
const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
export const ROW_PX = 52;
export const VIEW_HEIGHT_PX = 520;
const VIEW_WIDTH_PX = 360;
const SECOND_MS = 1000;

interface Filled {
  mailbox: Mailbox;
  cache: MailCache;
  online: boolean;
  client: QueryClient;
  openThreadId: string | null;
  onOpen: (row: ListRow) => void;
}

export interface Options {
  mailbox?: Mailbox | undefined;
  cache?: MailCache;
  online?: boolean;
  client?: QueryClient;
  openThreadId?: string | null;
  onOpen?: (row: ListRow) => void;
  // A component around the tree, for a probe that has to render with it.
  wrapper?: JSXElementConstructor<{ children: ReactNode }>;
}

export function inbox(): Mailbox {
  if (INBOX === undefined) {
    throw new Error("the fixtures hold no inbox");
  }
  return INBOX;
}

function tree({ mailbox, cache, online, client, openThreadId, onOpen }: Filled) {
  return (
    <QueryClientProvider client={client}>
      <ThreadList
        locale="en"
        today={TODAY}
        cache={cache}
        accountId={FASTMAIL.id}
        mailbox={mailbox}
        online={online}
        openThreadId={openThreadId}
        empty={<p>nothing here</p>}
        onOpen={onOpen}
      />
    </QueryClientProvider>
  );
}

function opensNothing(): void {
  // The rig's default: a row opened goes nowhere.
}

export function renderList(options: Options = {}) {
  const mailbox = options.mailbox ?? inbox();
  const filled: Filled = {
    mailbox,
    cache: options.cache ?? fixtureCache({ [`${mailbox.id}/0`]: INBOX_PAGE }),
    online: options.online ?? true,
    client: options.client ?? new QueryClient(),
    openThreadId: options.openThreadId ?? null,
    onOpen: options.onOpen ?? opensNothing,
  };
  const rendered = render(tree(filled), { wrapper: options.wrapper });
  return {
    rerender: (changes: Partial<Filled>) => {
      rendered.rerender(tree({ ...filled, ...changes }));
    },
  };
}

export function withPage(answer: PageAnswer): ReturnType<typeof fixtureCache> {
  return fixtureCache({ [`${INBOX_ID}/0`]: answer });
}

// The fixture pages, refused while `refused` says so.
export function refusableCache(state: { refused: boolean }): MailCache {
  const pages = fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE });
  return {
    ...pages,
    window: (accountId, mailboxId, page) =>
      state.refused
        ? Promise.reject(new JmapError("unavailable"))
        : pages.window(accountId, mailboxId, page),
  };
}

// The list over the real store and a fake server, so the rendered shape
// follows what the core lands; `poll` runs what the worker's poll runs.
export function coreCache(server: FakeJmap): MailCache & { poll: () => Promise<void> } {
  const client = new JmapClient(ACCOUNT);
  const store = new MemoryMailStore();
  vi.stubGlobal("fetch", server.fetch);
  return {
    mailboxes: () => Promise.resolve(MAILBOXES),
    window: async (_accountId, mailboxId, page) =>
      listPage(await queryWindow(client, store, mailboxId, page), mailboxId),
    thread: () => Promise.resolve(null),
    reveal: (accountId, mailboxId) => revealNewMail(store, accountId, mailboxId),
    poll: async () => {
      await applyChanges(client, store, [INBOX_ID]);
    },
  };
}

// `count` messages in the inbox on the server, one per thread, each a
// second apart; a negative step makes them older than the fixed day.
export function servedInbox(count: number, prefix: string, step: number): FakeJmap {
  const server = new FakeJmap();
  server.putMailbox(mailboxRow(INBOX_ID, "inbox"));
  addMail(server, count, prefix, step);
  return server;
}

export function addMail(server: FakeJmap, count: number, prefix: string, step: number): void {
  for (let index = 1; index <= count; index += 1) {
    const id = `${prefix}${String(index)}`;
    server.addEmail(email(id, { mailboxIds: [INBOX_ID], receivedAt: at(step * index) }));
  }
}

// A page of `count` plain rows numbered from `from`, in a list of `total`.
export function numbered(from: number, count: number, total: number): ListPage {
  const drafts: Draft[] = Array.from({ length: count }, (_, index) => ({
    sender: `Sender ${String(from + index)}`,
    subject: `Subject ${String(from + index)}`,
    preview: "",
    at: new Date(FIXED_NOW.getTime() - (from + index) * SECOND_MS),
  }));
  return { ...pageOf(drafts, 0, from), total };
}

export function rows(): HTMLElement[] {
  return within(screen.getByRole("grid")).getAllByRole("row");
}

export function row(index: number): HTMLElement {
  const found = rows().at(index);
  if (found === undefined) {
    throw new Error(`no row ${String(index)}`);
  }
  return found;
}

export function focused(): HTMLElement {
  const element = document.activeElement;
  if (!(element instanceof HTMLElement)) {
    throw new Error("nothing has focus");
  }
  return element;
}

export function press(key: string): void {
  fireEvent.keyDown(focused(), { key });
}

export function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

// One turn of the event loop, so a query's answer reaches the rendered tree.
export function settle(): Promise<void> {
  return act(
    () =>
      new Promise<void>((resolve) => {
        setTimeout(resolve, 0);
      }),
  );
}

// What the cache worker's message does: every page of the inbox refetches.
export function invalidateWindows(client: QueryClient): Promise<void> {
  return act(() =>
    client.invalidateQueries({ queryKey: queryKeys.windows(FASTMAIL.id, INBOX_ID) }),
  );
}

const layout = { wide: true };

// The scroll box has a size in the test, which jsdom gives no element; a
// box under a hidden ancestor has none, as in a browser. The width
// queries answer for the desktop layout, where the key hints show.
export function mockListBox(): void {
  beforeEach(() => {
    layout.wide = true;
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.closest("[hidden]") === null ? VIEW_HEIGHT_PX : 0;
    });
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(VIEW_WIDTH_PX);
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn<() => void>(),
    });
    stubWidthQueries(layout);
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("data-density");
  });
}
