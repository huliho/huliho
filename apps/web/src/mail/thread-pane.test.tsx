// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { MailCache, ThreadDetail } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { dispatchKey } from "../commands/registry";
import {
  FASTMAIL,
  INBOX_ID,
  MAILBOXES,
  THREAD,
  THREAD_ID,
  fixtureCache,
  threadDetail,
} from "./fixtures";
import type { ThreadAnswer } from "./fixtures";
import { ReadingPane, ThreadScreen } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import type { PanePosition } from "./thread-pane";

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
}

function renderPane(answer: ThreadAnswer = THREAD, options: Options = {}) {
  const onClose = vi.fn<() => void>();
  const cache = options.cache ?? fixtureCache({}, { [THREAD_ID]: answer });
  const keyHints = options.keyHints ?? true;
  const position = options.position ?? "right";
  const open = options.open ?? OPEN;
  render(
    <QueryClientProvider client={new QueryClient()}>
      {position === "screen" ? (
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
      )}
    </QueryClientProvider>,
  );
  return { onClose };
}

function cards(): HTMLElement[] {
  return screen.getAllByRole("listitem");
}

// The button that folds a card: the first one in it.
function headOf(card: HTMLElement): HTMLElement {
  const head = within(card).getAllByRole("button")[0];
  if (head === undefined) {
    throw new Error("the card has no head");
  }
  return head;
}

function command(key: string): void {
  act(() => {
    dispatchKey(new KeyboardEvent("keydown", { key }));
  });
}

afterEach(cleanup);

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
  expect(within(newest).getByText(NEWEST_TEXT)).toBeDefined();
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
    within(newest).getByText(NEWEST_TEXT),
    screen.getByText("sanne@fastmail.com"),
  ];
  expect(own.map((element) => element.getAttribute("dir"))).toEqual(own.map(() => "auto"));
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
  expect(back.textContent).toContain("Esc");
  fireEvent.click(back);
  expect(onClose).toHaveBeenCalledOnce();
  cleanup();
  renderPane(THREAD, { position: "screen", keyHints: false });
  await screen.findByRole("heading", { level: 1 });
  expect(screen.getByRole("button", { name: "Back to Inbox" }).textContent).not.toContain("Esc");
  cleanup();
  // A tree without the mailbox leaves the way back its bare word.
  renderPane(THREAD, { position: "screen", open: { ...OPEN, mailbox: undefined } });
  await screen.findByRole("heading", { level: 1 });
  expect(screen.getByRole("button", { name: "Back" }).textContent).toBe("BackEsc");
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
