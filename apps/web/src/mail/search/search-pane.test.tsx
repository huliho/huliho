// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { MethodFailure, SERVER_UNAVAILABLE, searchKeyOf } from "@huliho/core";
import type { ListPage, MailCache, SearchCall } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { installCommandListener } from "../../commands/registry";
import { Button } from "../../design-system/button";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { fixtureCache } from "../fixture-cache";
import { FASTMAIL, FIXED_NOW, INBOX_PAGE, MAILBOXES } from "../fixtures";
import { noteSearch, recentSearches } from "./recent-searches";
import { forgetRecentSearches } from "./recent-store";
import type { SearchRoute } from "./route-params";
import { SearchPane } from "./search-pane";
import { SEARCHING_AFTER_MS, SEARCH_SLOW_MS } from "./use-search";

const PARTS = { Button, EmptyState, ErrorState };
const RESULTS: ListPage = { ...INBOX_PAGE, total: 3 };
const EMPTY: ListPage = { rows: [], total: 0, pending: 0 };
const PIETER = "pieter@blom-installaties.example";

// What the search cache was asked, with each call's signal.
interface Asked {
  key: string;
  call: SearchCall;
}

// A cache whose search answers by key, holding a search it has no answer
// for until its signal aborts, as fetch does.
function searching(answers: Record<string, ListPage | Error>, asked: Asked[] = []): MailCache {
  return {
    ...fixtureCache({}),
    search: (_accountId, key, _page, call) => {
      asked.push({ key, call });
      const answer = new Map(Object.entries(answers)).get(key);
      if (answer === undefined) {
        return new Promise((_resolve, reject) => {
          call.signal?.addEventListener("abort", () => {
            reject(new DOMException("The search was given up.", "AbortError"));
          });
        });
      }
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
    },
  };
}

interface Options {
  route?: SearchRoute;
  online?: boolean;
  strict?: boolean;
  onClose?: () => void;
}

// The route the pane last moved to, as the address would show it.
let latest: SearchRoute = {};

// The pane over a route it keeps itself, as the route's view does.
function Harness({ cache, options }: { cache: MailCache; options: Options }) {
  const [route, setRoute] = useState<SearchRoute>(latest);
  return (
    <SearchPane
      locale="en"
      cache={cache}
      accountId={FASTMAIL.id}
      route={route}
      mailboxes={MAILBOXES}
      strict={options.strict ?? false}
      online={options.online ?? true}
      today={FIXED_NOW.getTime()}
      parts={PARTS}
      loading={<p>still rows</p>}
      onRoute={(next) => {
        latest = next;
        setRoute(next);
      }}
      onClose={options.onClose ?? (() => undefined)}
    />
  );
}

// Renders the pane and hands back a way to render it again with other
// options, over the route it holds.
function renderPane(cache: MailCache, options: Options = {}) {
  latest = options.route ?? { box: "mb-inbox" };
  const client = new QueryClient();
  const tree = (next: Options) => (
    <QueryClientProvider client={client}>
      <Harness cache={cache} options={next} />
    </QueryClientProvider>
  );
  const { rerender } = render(tree(options));
  return (next: Options): void => {
    rerender(tree(next));
  };
}

const HINT = "Words, a “quoted phrase” or an operator such as from: or in:";

const field = () => screen.getByRole("combobox", { name: "Search mail" });
const route = () => latest;
const region = () => within(screen.getByRole("search")).getByRole("status");
const shortcut = () => screen.queryByRole("button", { name: "from: a sender" });
// The element the field's description points at.
const described = () => document.getElementById(field().getAttribute("aria-describedby") ?? "");

function type(text: string): void {
  fireEvent.change(field(), { target: { value: text } });
}

function press(key: string, target: HTMLElement = field()): void {
  fireEvent.keyDown(target, { key });
}

let uninstall: () => void = () => undefined;

beforeEach(() => {
  uninstall = installCommandListener();
});

afterEach(() => {
  uninstall();
  cleanup();
  forgetRecentSearches();
  localStorage.clear();
  vi.useRealTimers();
});

test("the field takes the focus as the search opens, with the hint and the shortcuts", () => {
  renderPane(searching({}));
  expect(document.activeElement).toBe(field());
  expect(screen.getByRole("search")).toBeDefined();
  expect(screen.getAllByText(HINT).length).toBeGreaterThan(0);
  expect(
    screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter((text) => text.includes(":")),
  ).toEqual(["from:", "in:", "is:unread", "has:attachment"]);
});

test("the hint stands once and describes the field, before the first run and after it", async () => {
  renderPane(searching({ [searchKeyOf({ q: "pie" })]: RESULTS }));
  expect(screen.getAllByText(HINT)).toHaveLength(1);
  expect(described()?.textContent).toBe(HINT);
  type("pie");
  press("Enter");
  await waitFor(() => {
    expect(region().textContent).toBe("3 results for “pie” in all mailboxes");
  });
  expect(screen.getAllByText(HINT)).toHaveLength(1);
  expect(described()?.textContent).toBe(HINT);
});

test("typing offers the plain search and the senders; Enter runs the text and keeps the box", async () => {
  const asked: Asked[] = [];
  renderPane(searching({ [searchKeyOf({ q: "pie" })]: RESULTS }, asked));
  type("pie");
  expect(await screen.findByRole("option", { name: "Search for “pie”" })).toBeDefined();
  expect(await screen.findByRole("option", { name: `from: Pieter Blom, ${PIETER}` })).toBeDefined();
  press("Enter");
  expect(route()).toEqual({ q: "pie", box: "mb-inbox" });
  await waitFor(() => {
    expect(region().textContent).toBe("3 results for “pie” in all mailboxes");
  });
  expect(asked.some(({ call }) => call.again === true)).toBe(true);
  expect(recentSearches(FASTMAIL.id, false)).toEqual([searchKeyOf({ q: "pie" })]);
});

test("a sender picked from the suggestions becomes a token and runs at once", async () => {
  renderPane(searching({ [searchKeyOf({ from: PIETER })]: RESULTS }));
  type("from:pie");
  fireEvent.click(await screen.findByRole("option", { name: /Pieter Blom/ }));
  expect(route()).toEqual({ from: PIETER, box: "mb-inbox" });
  expect(await screen.findByRole("button", { name: "Remove from: Pieter Blom" })).toBeDefined();
  expect(field()).toHaveProperty("value", "");
  await waitFor(() => {
    expect(region().textContent).toBe("3 results in all mailboxes");
  });
});

test("an operator typed with a space after it becomes a token; Backspace at the start takes it", () => {
  renderPane(searching({}));
  type("has:attachment ");
  expect(screen.getByRole("button", { name: "Remove has: attachment" })).toBeDefined();
  expect(field()).toHaveProperty("value", "");
  press("Backspace");
  expect(screen.queryByRole("button", { name: "Remove has: attachment" })).toBeNull();
});

test("the shortcuts write an operator or set one and leave the focus in the field", () => {
  renderPane(searching({}));
  fireEvent.click(screen.getByRole("button", { name: "from: a sender" }));
  expect(field()).toHaveProperty("value", "from:");
  expect(document.activeElement).toBe(field());
  fireEvent.click(screen.getByRole("button", { name: "has:attachment, with attachments only" }));
  expect(screen.getByRole("button", { name: "Remove has: attachment" })).toBeDefined();
  expect(field()).toHaveProperty("value", "from:");
});

test("Escape clears a field that holds something with its tokens and closes an empty one", () => {
  const onClose = vi.fn<() => void>();
  renderPane(searching({}), { onClose });
  type("has:attachment offerte");
  expect(screen.getByRole("button", { name: "Remove has: attachment" })).toBeDefined();
  press("Escape");
  expect(field()).toHaveProperty("value", "");
  expect(screen.queryByRole("button", { name: "Remove has: attachment" })).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  press("Escape");
  expect(onClose).toHaveBeenCalledOnce();
});

test("Escape outside the field closes the search, `/` puts the focus back in the field", () => {
  const onClose = vi.fn<() => void>();
  renderPane(searching({}), { onClose });
  const inside = screen.getByRole("button", { name: "in: a mailbox" });
  inside.focus();
  press("/", inside);
  expect(document.activeElement).toBe(field());
  inside.focus();
  press("Escape", inside);
  expect(onClose).toHaveBeenCalledOnce();
});

test("a click on Close closes the search", () => {
  const onClose = vi.fn<() => void>();
  renderPane(searching({}), { onClose });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledOnce();
});

test("a search in flight says searching, then slow with Cancel, which aborts it", async () => {
  vi.useFakeTimers();
  const asked: Asked[] = [];
  renderPane(searching({}, asked), { route: { q: "offerte" } });
  expect(screen.getByText("still rows")).toBeDefined();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(SEARCHING_AFTER_MS);
  });
  expect(region().textContent).toBe("Searching…");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(SEARCH_SLOW_MS - SEARCHING_AFTER_MS);
  });
  expect(region().textContent).toBe(
    "Still searching. A server without a search index can take a while.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(asked[0]?.call.signal?.aborted).toBe(true);
  expect(region().textContent).toBe("Search canceled.");
  expect(field()).toHaveProperty("value", "offerte");
  expect(document.activeElement).toBe(field());
});

test("a press under the field keeps the shortcuts in place until its click lands", async () => {
  renderPane(searching({ [searchKeyOf({ q: "offerte" })]: RESULTS }), {
    route: { q: "offerte", box: "mb-inbox" },
  });
  const widen = await screen.findByRole("button", { name: "Include Junk and Trash" });
  expect(shortcut()).not.toBeNull();
  fireEvent.pointerDown(widen);
  act(() => {
    widen.focus();
  });
  expect(shortcut()).not.toBeNull();
  fireEvent.click(widen);
  await waitFor(() => {
    expect(shortcut()).toBeNull();
  });
  expect(route()).toEqual({ q: "offerte", in: "all", box: "mb-inbox" });
});

test("results count in the slot and widen to Junk and Trash", async () => {
  renderPane(searching({ [searchKeyOf({ q: "offerte" })]: RESULTS }), {
    route: { q: "offerte", box: "mb-inbox" },
  });
  await waitFor(() => {
    expect(region().textContent).toBe("3 results for “offerte” in all mailboxes");
  });
  fireEvent.click(screen.getByRole("button", { name: "Include Junk and Trash" }));
  expect(route()).toEqual({ q: "offerte", in: "all", box: "mb-inbox" });
});

test("no match shows the empty state with its hint and the widening", async () => {
  renderPane(searching({ [searchKeyOf({ q: "zzz", in: "mb-facturen" })]: EMPTY }), {
    route: { q: "zzz", in: "mb-facturen" },
  });
  expect(await screen.findByText("No mail matches “zzz”", { selector: "p" })).toBeDefined();
  expect(region().textContent).toBe("No mail matches “zzz”");
  expect(screen.getByText("Check the spelling or search all mailboxes.")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Search all mailboxes" }));
  expect(route()).toEqual({ q: "zzz" });
});

test("the deadline reads its own sentence and Try again asks the server anew", async () => {
  const asked: Asked[] = [];
  const late = new MethodFailure(SERVER_UNAVAILABLE, "Email/query");
  renderPane(searching({ [searchKeyOf({ q: "offerte" })]: late }, asked), {
    route: { q: "offerte" },
  });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain(
    "The search took too long. Narrow it to one mailbox or add a filter.",
  );
  expect(region().textContent).toBe("");
  fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
  await waitFor(() => {
    expect(asked.filter(({ call }) => call.again === true)).toHaveLength(1);
  });
});

test("any other failure reads the plain sentence", async () => {
  renderPane(searching({ [searchKeyOf({ q: "offerte" })]: new TypeError("offline") }), {
    route: { q: "offerte" },
  });
  expect((await screen.findByRole("alert")).textContent).toContain("Couldn’t search. Try again.");
});

test("offline the search asks nothing, joins no recents and says it needs a connection", () => {
  const asked: Asked[] = [];
  renderPane(searching({}, asked), { route: { q: "offerte" }, online: false });
  expect(screen.getByText("Search needs a connection.", { selector: "p" })).toBeDefined();
  expect(region().textContent).toBe("Search needs a connection.");
  type("dakkapel");
  press("Enter");
  expect(asked).toEqual([]);
  expect(recentSearches(FASTMAIL.id, false)).toEqual([]);
});

test("a search typed offline joins the recents once the pane is online", async () => {
  const asked: Asked[] = [];
  const again = renderPane(searching({}, asked), { route: { q: "offerte" }, online: false });
  type("dakkapel");
  press("Enter");
  expect(route()).toEqual({ q: "dakkapel" });
  expect(recentSearches(FASTMAIL.id, false)).toEqual([]);
  again({ online: true });
  expect(recentSearches(FASTMAIL.id, false)).toEqual([searchKeyOf({ q: "dakkapel" })]);
  await waitFor(() => {
    expect(asked.map(({ key }) => key)).toEqual([searchKeyOf({ q: "dakkapel" })]);
  });
});

test("Enter on the search the route holds moves it to the front of the recents", async () => {
  const offerte = searchKeyOf({ q: "offerte" });
  const dakkapel = searchKeyOf({ q: "dakkapel" });
  const asked: Asked[] = [];
  renderPane(searching({ [offerte]: RESULTS }, asked), { route: { q: "offerte" } });
  await waitFor(() => {
    expect(region().textContent).toBe("3 results for “offerte” in all mailboxes");
  });
  noteSearch(FASTMAIL.id, dakkapel, false);
  expect(recentSearches(FASTMAIL.id, false)).toEqual([dakkapel, offerte]);
  press("Enter");
  expect(recentSearches(FASTMAIL.id, false)).toEqual([offerte, dakkapel]);
  await waitFor(() => {
    expect(asked.filter(({ call }) => call.again === true)).toHaveLength(1);
  });
});

test("the recents stand under the empty field and a pick runs one", async () => {
  noteSearch(FASTMAIL.id, searchKeyOf({ q: "factuur", unread: true }), false);
  renderPane(searching({ [searchKeyOf({ q: "factuur", unread: true })]: RESULTS }));
  expect(screen.getByText("Recent")).toBeDefined();
  fireEvent.click(screen.getByRole("option", { name: "is: unread factuur" }));
  expect(route()).toEqual({ q: "factuur", unread: "1", box: "mb-inbox" });
  await waitFor(() => {
    expect(region().textContent).toBe("3 results for “factuur” in all mailboxes");
  });
});

test("a recent marks the typed letters in place past a character whose lowercase form is longer", async () => {
  noteSearch(FASTMAIL.id, searchKeyOf({ q: "İstanbul" }), false);
  renderPane(searching({}));
  type("stanbul");
  const recent = await screen.findByRole("option", { name: "İstanbul" });
  expect(within(recent).getByText("stanbul")).toBeDefined();
  expect(recent.textContent.trim()).toBe("İstanbul");
});

test("under strict a search joins the recents in memory alone", () => {
  renderPane(searching({}), { strict: true });
  type("offerte");
  press("Enter");
  expect(localStorage.length).toBe(0);
  expect(recentSearches(FASTMAIL.id, true)).toEqual([searchKeyOf({ q: "offerte" })]);
});
