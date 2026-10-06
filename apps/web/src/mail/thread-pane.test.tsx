// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { EmailHeader, MailCache, ThreadDetail } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import type { JSX } from "react";

import { dispatchKey } from "../commands/registry";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { fixtureCache } from "./fixture-cache";
import type { ThreadAnswer } from "./fixture-cache";
import { FASTMAIL, INBOX_ID, MAILBOXES, THREAD, THREAD_ID, threadDetail } from "./fixtures";
import { ReadingPane, ThreadScreen, prefetchThreadPane } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import type { PanePosition } from "./thread-pane";

// The pane's code is in before the first render, as the shell has it.
beforeAll(async () => {
  await prefetchThreadPane();
});

const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
const OPEN: OpenThread = { accountId: FASTMAIL.id, threadId: THREAD_ID, mailbox: INBOX };
const SUBJECT = "Offerte badkamerrenovatie, herziene versie";
const COUNT = 14;
// The two collapsed cards in sight and the newest, open.
const IN_SIGHT = 3;
// The text of the newest message, the one sentence that stands for its body.
const NEWEST_TEXT = "De meerprijs voor de vloerverwarming ontbreekt nog.";
const UNREAD_AT = 4;

interface Options {
  position?: PanePosition;
  keyHints?: boolean;
  cache?: MailCache;
  open?: OpenThread;
  // The query client of an earlier render, for a thread opened again.
  client?: QueryClient;
}

// The pane in a router of its own, since a card's hooks reach for the
// session's end.
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

function renderPane(answer: ThreadAnswer = THREAD, options: Options = {}) {
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

function cards(): HTMLElement[] {
  return screen.getAllByRole("listitem");
}

function expandedOf(card: HTMLElement | undefined): string | null {
  return card === undefined ? null : headOf(card).getAttribute("aria-expanded");
}

// The open thread as the cache hands it on after a change.
function landIn(client: QueryClient, detail: ThreadDetail): void {
  act(() => {
    client.setQueryData(queryKeys.thread(FASTMAIL.id, THREAD_ID), detail);
  });
}

// The fixture thread with messages after its newest, each read or unread.
function withLanded(landed: readonly (readonly [id: string, unread: boolean])[]): ThreadDetail {
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

// The button that folds a card: the first one in it.
function headOf(card: HTMLElement): HTMLElement {
  const head = within(card).getAllByRole("button")[0];
  if (head === undefined) {
    throw new Error("the card has no head");
  }
  return head;
}

// Presses the head of the newest card.
function pressNewest(): void {
  const card = cards().at(-1);
  if (card === undefined) {
    throw new Error("the pane has no card");
  }
  fireEvent.click(headOf(card));
}

function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

// The width queries answer for the desktop layout; the card reads the theme through one.
beforeEach(() => {
  stubWidthQueries({ wide: true });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("the pane shows the subject, the count, the older button and the cards in sight, the newest open", async () => {
  renderPane();
  const title = await screen.findByRole("heading", { level: 2, name: SUBJECT });
  expect(screen.getByRole("complementary", { name: "Conversation" })).toBeDefined();
  expect(screen.getByText("14 messages")).toBeDefined();
  expect(screen.getByRole("button", { name: "Show 11 older messages" })).toBeDefined();
  expect(cards()).toHaveLength(IN_SIGHT);
  expect(cards().map((card) => headOf(card).getAttribute("aria-expanded"))).toEqual([
    "false",
    "false",
    "true",
  ]);
  const newest = cards()[2];
  if (newest === undefined) {
    throw new Error("no newest card");
  }
  expect(within(newest).getByText("Pieter Blom")).toBeDefined();
  expect(within(newest).getByText("pieter@blom-installaties.example")).toBeDefined();
  expect(within(newest).getByText("to Sanne Bakker")).toBeDefined();
  expect(await within(newest).findByText(NEWEST_TEXT)).toBeDefined();
  expect(headOf(newest).textContent).toContain("8:15");
  expect(within(newest).getByRole("button", { name: "Show all recipients" })).toBeDefined();
  // A collapsed card is one line: who, its text and a short time.
  const collapsed = cards()[1];
  expect(collapsed?.textContent).toContain("Versie 2 staat in de bijlage, met het tegelwerk erin.");
  // A collapsed card dates itself the way a row does: by the day, once it is not today's.
  expect(collapsed?.textContent).toContain("May 14");
  expect(collapsed?.textContent).not.toContain("3:15");
  expect(document.activeElement).toBe(title);
});

test("a collapsed card opens on its head and closes again; the older ones come on their button", async () => {
  renderPane();
  await screen.findByRole("heading", { level: 2 });
  const collapsed = cards()[0];
  if (collapsed === undefined) {
    throw new Error("no collapsed card");
  }
  fireEvent.click(headOf(collapsed));
  expect(headOf(collapsed).getAttribute("aria-expanded")).toBe("true");
  expect(within(collapsed).getByText("to Sanne Bakker")).toBeDefined();
  fireEvent.click(headOf(collapsed));
  expect(headOf(collapsed).getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "Show 11 older messages" }));
  expect(cards()).toHaveLength(COUNT);
  expect(screen.queryByRole("button", { name: /older messages/ })).toBeNull();
  const first = cards()[0];
  expect(first !== undefined && document.activeElement).toBe(
    first === undefined ? null : headOf(first),
  );
  expect(first?.textContent).toContain("Hierbij de eerste versie van de offerte voor de badkamer.");
});

test("the subject, who wrote, the recipients and the text each read in their own direction", async () => {
  renderPane();
  const title = await screen.findByRole("heading", { level: 2, name: SUBJECT });
  const newest = cards()[2];
  if (newest === undefined) {
    throw new Error("no newest card");
  }
  fireEvent.click(screen.getByRole("button", { name: "Show all recipients" }));
  const own = [
    title,
    within(newest).getByText("Pieter Blom"),
    within(newest).getByText("pieter@blom-installaties.example"),
    within(newest).getByText("to Sanne Bakker"),
    (await within(newest).findByText(NEWEST_TEXT)).closest("[dir]"),
    screen.getByText("sanne@fastmail.com"),
  ];
  expect(own.map((element) => element?.getAttribute("dir"))).toEqual(own.map(() => "auto"));
});

test("every recipient shows by field with the address beside the name", async () => {
  renderPane();
  await screen.findByRole("heading", { level: 2 });
  fireEvent.click(screen.getByRole("button", { name: "Show all recipients" }));
  const list = document.querySelector("dl");
  expect(list?.textContent).toContain("To");
  expect(list?.textContent).toContain("Sanne Bakker");
  expect(list?.textContent).toContain("sanne@fastmail.com");
  expect(list?.textContent).toContain("Cc");
  expect(list?.textContent).toContain("Jonas Verhulst");
  expect(list?.textContent).not.toContain("Bcc");
  const hide = screen.getByRole("button", { name: "Hide recipients" });
  expect(hide.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(hide);
  expect(screen.queryByRole("button", { name: "Hide recipients" })).toBeNull();
});

test("an unread message opens in sight and says so to a screen reader", async () => {
  renderPane(threadDetail([UNREAD_AT]));
  await screen.findByRole("heading", { level: 2 });
  expect(screen.getByRole("button", { name: "Show 2 older messages" })).toBeDefined();
  expect(cards()).toHaveLength(COUNT - 2);
  const unread = cards()[UNREAD_AT - 2];
  expect(unread?.dataset["unread"]).toBe("true");
  expect(unread === undefined ? null : headOf(unread).getAttribute("aria-expanded")).toBe("true");
  expect(unread === undefined ? null : headOf(unread).textContent).toContain("Unread");
});

test("the cards in sight and the open ones stay as first drawn once the unread one reads as read", async () => {
  const { client } = renderPane(threadDetail([UNREAD_AT]));
  await screen.findByRole("heading", { level: 2 });
  const drawn = cards().map(expandedOf);
  landIn(client, THREAD);
  await waitFor(() => {
    expect(cards()[UNREAD_AT - 2]?.dataset["unread"]).toBeUndefined();
  });
  expect(screen.getByRole("button", { name: "Show 2 older messages" })).toBeDefined();
  expect(cards()).toHaveLength(COUNT - 2);
  expect(cards().map(expandedOf)).toEqual(drawn);
  expect(expandedOf(cards()[UNREAD_AT - 2])).toBe("true");
});

test("a message that lands in the open thread comes in folded and stays unread until its head opens it", async () => {
  const cache = fixtureCache({}, { [THREAD_ID]: THREAD });
  const { client } = renderPane(THREAD, { cache });
  await screen.findByRole("heading", { level: 2 });
  landIn(client, withLanded([["e-landed", true]]));
  await waitFor(() => {
    expect(cards()).toHaveLength(IN_SIGHT + 1);
  });
  const landed = cards().at(IN_SIGHT);
  if (landed === undefined) {
    throw new Error("no landed card");
  }
  expect(landed.dataset["unread"]).toBe("true");
  expect(expandedOf(landed)).toBe("false");
  // The card that stood open at first stays open.
  expect(expandedOf(cards().at(IN_SIGHT - 1))).toBe("true");
  expect(cache.mutations).toEqual([]);
  fireEvent.click(headOf(landed));
  expect(expandedOf(cards().at(IN_SIGHT))).toBe("true");
  await waitFor(() => {
    expect(cache.mutations).toEqual([
      { type: "Email", id: "e-landed", patch: { "keywords/$seen": true } },
    ]);
  });
  // It reads as read and a newer message stands after it, folded in its turn.
  landIn(
    client,
    withLanded([
      ["e-landed", false],
      ["e-newer", true],
    ]),
  );
  await waitFor(() => {
    expect(cards()).toHaveLength(IN_SIGHT + 2);
  });
  expect(cards().at(IN_SIGHT)?.dataset["unread"]).toBeUndefined();
  expect(expandedOf(cards().at(IN_SIGHT))).toBe("true");
  expect(expandedOf(cards().at(IN_SIGHT + 1))).toBe("false");
  expect(cache.mutations).toHaveLength(1);
});

test("a message that turns unread under its open card stays unread until its head opens it again", async () => {
  const cache = fixtureCache({}, { [THREAD_ID]: THREAD });
  const { client } = renderPane(THREAD, { cache });
  await screen.findByRole("heading", { level: 2 });
  const newest = Object.values(THREAD.emails).at(-1);
  if (newest === undefined) {
    throw new Error("the fixture thread is empty");
  }
  // Another client took the mark away and a poll lands the change.
  landIn(client, {
    thread: THREAD.thread,
    emails: { ...THREAD.emails, [newest.id]: { ...newest, keywords: {} } },
  });
  await waitFor(() => {
    expect(cards().at(-1)?.dataset["unread"]).toBe("true");
  });
  expect(expandedOf(cards().at(-1))).toBe("true");
  expect(cache.mutations).toEqual([]);
  // Folded and opened again by its head.
  pressNewest();
  pressNewest();
  await waitFor(() => {
    expect(cache.mutations).toEqual([
      { type: "Email", id: newest.id, patch: { "keywords/$seen": true } },
    ]);
  });
});

// The fixture thread opened, closed and named by a poll, then opened
// again on the same query client with the cache answering `next`.
async function reopened(next: ThreadDetail | Error): Promise<ReturnType<typeof fixtureCache>> {
  const held = fixtureCache({}, { [THREAD_ID]: THREAD });
  const thread = vi.fn<() => Promise<ThreadDetail | null>>().mockResolvedValueOnce(THREAD);
  const cache = { ...held, thread };
  const { client } = renderPane(THREAD, { cache });
  await screen.findByRole("heading", { level: 2 });
  cleanup();
  await act(() => client.invalidateQueries({ queryKey: queryKeys.thread(FASTMAIL.id, THREAD_ID) }));
  if (next instanceof Error) {
    thread.mockRejectedValue(next);
  } else {
    thread.mockResolvedValue(next);
  }
  renderPane(THREAD, { cache, client });
  return held;
}

test("a thread opened again opens the reply that arrived meanwhile and marks it read", async () => {
  const cache = await reopened(withLanded([["e-landed", true]]));
  await waitFor(() => {
    expect(cache.mutations).toEqual([
      { type: "Email", id: "e-landed", patch: { "keywords/$seen": true } },
    ]);
  });
  expect(cards().map(expandedOf)).toEqual(["false", "false", "true"]);
  expect(cards().at(-1)?.dataset["unread"]).toBe("true");
});

test("a thread opened again draws as it was read before when the fresh read fails, with no fault shown", async () => {
  const cache = await reopened(new JmapError("unavailable"));
  expect(await screen.findByRole("heading", { level: 2, name: SUBJECT })).toBeDefined();
  expect(cards().map(expandedOf)).toEqual(["false", "false", "true"]);
  expect(cache.mutations).toEqual([]);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("Escape and the Close button close the thread", async () => {
  const { onClose } = renderPane();
  await screen.findByRole("heading", { level: 2 });
  command("Escape");
  expect(onClose).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Close conversation" }));
  expect(onClose).toHaveBeenCalledTimes(2);
});

test("as a screen the title is the page's heading and the way back names the mailbox with its key", async () => {
  const { onClose } = renderPane(THREAD, { position: "screen" });
  const title = await screen.findByRole("heading", { level: 1, name: SUBJECT });
  expect(screen.getByRole("region", { name: "Conversation" })).toBeDefined();
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(document.activeElement).toBe(title);
  const back = screen.getByRole("button", { name: "Back to Inbox" });
  expect(back.textContent).toContain("Inbox");
  expect(back.textContent).toContain("esc");
  fireEvent.click(back);
  expect(onClose).toHaveBeenCalledOnce();
  cleanup();
  renderPane(THREAD, { position: "screen", keyHints: false });
  await screen.findByRole("heading", { level: 1 });
  expect(screen.getByRole("button", { name: "Back to Inbox" }).textContent).not.toContain("esc");
  cleanup();
  // A tree without the mailbox leaves the way back its bare word.
  renderPane(THREAD, { position: "screen", open: { ...OPEN, mailbox: undefined } });
  await screen.findByRole("heading", { level: 1 });
  expect(screen.getByRole("button", { name: "Back" }).textContent).toBe("Backesc");
});

test("the thread loads, fails with Try again and says when it is gone", async () => {
  renderPane("never");
  expect(await screen.findByRole("status", { name: "Loading…" })).toBeDefined();
  cleanup();
  const thread = vi
    .fn<() => Promise<ThreadDetail | null>>()
    .mockRejectedValueOnce(new JmapError("unavailable"))
    .mockResolvedValue(THREAD);
  renderPane(THREAD, { cache: { ...fixtureCache({}), thread } });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Couldn’t load your mail.");
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { level: 2, name: SUBJECT })).toBeDefined();
  cleanup();
  renderPane(null);
  expect(
    await screen.findByText("Couldn’t find this conversation. It may have been moved or deleted."),
  ).toBeDefined();
});
