// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, MailCache, ThreadDetail } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, vi } from "vitest";
import type { JSX } from "react";

import { dispatchKey } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { StillObserver } from "./card-rig";
import { fixtureCache } from "./fixture-cache";
import type { ThreadAnswer } from "./fixture-cache";
import { FASTMAIL, INBOX_ID, MAILBOXES, THREAD, THREAD_ID } from "./fixtures";
import { prefetchInspector } from "./message-card";
import { ReadingPane, ThreadScreen, prefetchThreadPane } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import type { PanePosition } from "./thread-pane";

// The rig the pane tests render in: the pane or the screen over a
// fixture cache, in a router of its own, since a card's hooks reach
// for the session's end.

const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
export const OPEN: OpenThread = { accountId: FASTMAIL.id, threadId: THREAD_ID, mailbox: INBOX };
export const SUBJECT = "Offerte badkamerrenovatie, herziene versie";
export const COUNT = 14;
// The two collapsed cards in sight and the newest, open.
export const IN_SIGHT = 3;
// The text of the newest message, the one sentence that stands for its body.
export const NEWEST_TEXT = "De meerprijs voor de vloerverwarming ontbreekt nog.";

export interface PaneOptions {
  position?: PanePosition;
  keyHints?: boolean;
  cache?: MailCache;
  open?: OpenThread;
  // The query client of an earlier render, for a thread opened again.
  client?: QueryClient;
}

function routed(Screen: () => JSX.Element, client: QueryClient): JSX.Element {
  const router = createRouter({
    routeTree: createRootRoute({ component: Screen }),
    history: createMemoryHistory({ initialEntries: ["/mail/acc-1/mb-inbox"] }),
  });
  return (
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

export function renderPane(answer: ThreadAnswer = THREAD, options: PaneOptions = {}) {
  const onClose = vi.fn<() => void>();
  const client = options.client ?? new QueryClient();
  const cache = options.cache ?? fixtureCache({}, { [THREAD_ID]: answer });
  const keyHints = options.keyHints ?? true;
  const position = options.position ?? "right";
  const open = options.open ?? OPEN;
  render(
    routed(
      () =>
        position === "screen" ? (
          <ThreadScreen
            locale="en"
            cache={cache}
            thread={open}
            keyHints={keyHints}
            onClose={onClose}
          />
        ) : (
          <ReadingPane
            locale="en"
            cache={cache}
            thread={open}
            position={position}
            keyHints={keyHints}
            onClose={onClose}
          />
        ),
      client,
    ),
  );
  return { onClose, client };
}

// The cards of the thread: the items of its own list, oldest first.
export function cards(): HTMLElement[] {
  return screen.getAllByRole("listitem").filter((item) => item.dataset["messageId"] !== undefined);
}

// The button that folds a card: the first one in it.
export function headOf(card: HTMLElement): HTMLElement {
  const head = within(card).getAllByRole("button")[0];
  if (head === undefined) {
    throw new Error("the card has no head");
  }
  return head;
}

export function expandedOf(card: HTMLElement | undefined): string | null {
  return card === undefined ? null : headOf(card).getAttribute("aria-expanded");
}

// The open thread as the cache hands it on after a change.
export function landIn(client: QueryClient, detail: ThreadDetail): void {
  act(() => {
    client.setQueryData(queryKeys.thread(FASTMAIL.id, THREAD_ID), detail);
  });
}

// The fixture thread with messages after its newest, each read or unread.
export function withLanded(
  landed: readonly (readonly [id: string, unread: boolean])[],
): ThreadDetail {
  const newest = Object.values(THREAD.emails).at(-1);
  if (newest === undefined) {
    throw new Error("the fixture thread is empty");
  }
  const added = landed.map(([id, unread]): EmailHeader => ({
    ...newest,
    id,
    blobId: id,
    keywords: unread ? {} : { $seen: true },
  }));
  return {
    thread: {
      ...THREAD.thread,
      emailIds: [...THREAD.thread.emailIds, ...added.map((email) => email.id)],
    },
    emails: { ...THREAD.emails, ...Object.fromEntries(added.map((email) => [email.id, email])) },
  };
}

// Presses the head of the newest card.
export function pressNewest(): void {
  const card = cards().at(-1);
  if (card === undefined) {
    throw new Error("the pane has no card");
  }
  fireEvent.click(headOf(card));
}

// A key as the registry hears it from the app's window.
export function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

// The pane's code and the inspector's in before the first render, as
// the shell and an open card have them; the width queries answer for
// the desktop layout; the card reads the theme through one and a frame
// watches its document with an observer.
export function mockPaneBox(): void {
  beforeAll(async () => {
    await Promise.all([prefetchThreadPane(), prefetchInspector()]);
  });
  beforeEach(() => {
    stubWidthQueries({ wide: true });
    vi.stubGlobal("ResizeObserver", StillObserver);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });
}
