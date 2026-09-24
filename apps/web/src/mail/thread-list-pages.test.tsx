// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { WINDOW_SIZE } from "@huliho/core";
import type { MailCache, WindowPage } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { fireEvent, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { FASTMAIL, INBOX_ID, INBOX_PAGE, fixtureCache } from "./fixtures";
import {
  ROW_PX,
  VIEW_HEIGHT_PX,
  addMail,
  coreCache,
  focused,
  invalidateWindows,
  mockListBox,
  numbered,
  press,
  refusableCache,
  renderList,
  row,
  rows,
  servedInbox,
  settle,
} from "./list-rig";

// A total past one page, so the rows of a second page load by scrolling.
const LONG_TOTAL = WINDOW_SIZE + 5;
const ROWS_IN_VIEW = VIEW_HEIGHT_PX / ROW_PX;
// A list whose second page comes short of a full one.
const SHORT_SECOND = WINDOW_SIZE + 25;

mockListBox();

test("the grid comes with its rows once the first page lands", async () => {
  const first = Promise.withResolvers<WindowPage>();
  const pages = fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE });
  const window = vi.fn<MailCache["window"]>(() => first.promise);
  renderList({ cache: { ...pages, window } });
  await screen.findByRole("status", { name: "Loading…" });
  expect(screen.queryByRole("grid")).toBeNull();
  first.resolve(INBOX_PAGE);
  await screen.findByRole("grid");
  expect(rows().length).toBeGreaterThan(0);
});

test("a second page loads when its rows come into view; its rows are still until then", async () => {
  const second = Promise.withResolvers<WindowPage>();
  const pages = fixtureCache({ [`${INBOX_ID}/0`]: numbered(0, WINDOW_SIZE, LONG_TOTAL) });
  const window = vi.fn<MailCache["window"]>((accountId, mailboxId, page) =>
    page === 1 ? second.promise : pages.window(accountId, mailboxId, page),
  );
  renderList({ cache: { ...pages, window } });
  const grid = await screen.findByRole("grid");
  expect(grid.getAttribute("aria-rowcount")).toBe(String(LONG_TOTAL));
  expect(window).toHaveBeenCalledTimes(1);
  row(0).focus();
  press("End");
  await vi.waitFor(() => {
    expect(window).toHaveBeenCalledWith(FASTMAIL.id, INBOX_ID, 1);
  });
  const last = rows().at(-1);
  expect(last?.getAttribute("aria-rowindex")).toBe(String(LONG_TOTAL));
  expect(last?.getAttribute("aria-busy")).toBe("true");
  expect(focused()).toBe(last);
  second.resolve(numbered(WINDOW_SIZE, LONG_TOTAL - WINDOW_SIZE, LONG_TOTAL));
  await vi.waitFor(() => {
    expect(rows().at(-1)?.getAttribute("aria-label")).toContain(`Sender ${String(LONG_TOTAL - 1)}`);
  });
});

test("a second page held short fills up once the poll finds rows behind it", async () => {
  const server = servedInbox(SHORT_SECOND, "e", 1);
  const cache = coreCache(server);
  const client = new QueryClient();
  renderList({ cache, client });
  await screen.findByRole("grid");
  row(0).focus();
  press("End");
  await vi.waitFor(() => {
    expect(rows().at(-1)?.getAttribute("aria-label")).toContain("Message e1,");
  });
  addMail(server, SHORT_SECOND, "o", -1);
  await cache.poll();
  await invalidateWindows(client);
  await vi.waitFor(() => {
    expect(screen.getByRole("grid").getAttribute("aria-rowcount")).toBe(String(SHORT_SECOND * 2));
  });
  press("End");
  await vi.waitFor(() => {
    expect(rows().at(-1)?.getAttribute("aria-label")).toContain(
      `Message o${String(SHORT_SECOND)},`,
    );
  });
  expect(rows().at(-1)?.getAttribute("aria-rowindex")).toBe(String(SHORT_SECOND * 2));
});

test("a shown row that leaves while new mail waits keeps every row reachable", async () => {
  const server = servedInbox(SHORT_SECOND, "e", 1);
  const cache = coreCache(server);
  const client = new QueryClient();
  renderList({ cache, client });
  await screen.findByRole("grid");
  row(0).focus();
  press("End");
  await vi.waitFor(() => {
    expect(rows().at(-1)?.getAttribute("aria-label")).toContain("Message e1,");
  });
  server.destroyEmail("e50");
  addMail(server, 1, "n", SHORT_SECOND + 1);
  await cache.poll();
  await invalidateWindows(client);
  await vi.waitFor(() => {
    expect(screen.getByRole("grid").getAttribute("aria-rowcount")).toBe(String(SHORT_SECOND - 1));
  });
  await vi.waitFor(() => {
    const last = rows().at(-1);
    expect(last?.getAttribute("aria-rowindex")).toBe(String(SHORT_SECOND - 1));
    expect(last?.getAttribute("aria-label")).toContain("Message e1,");
  });
  expect(rows().some((it) => it.getAttribute("aria-busy") === "true")).toBe(false);
});

test("no page past the last row is asked for", async () => {
  const pages = fixtureCache({ [`${INBOX_ID}/0`]: numbered(0, WINDOW_SIZE, WINDOW_SIZE) });
  const window = vi.fn<MailCache["window"]>((accountId, mailboxId, page) =>
    pages.window(accountId, mailboxId, page),
  );
  renderList({ cache: { ...pages, window } });
  const grid = await screen.findByRole("grid");
  // The last rows in view: the lookahead past them would reach a second page.
  grid.scrollTop = (WINDOW_SIZE - ROWS_IN_VIEW) * ROW_PX;
  fireEvent.scroll(grid);
  await vi.waitFor(() => {
    expect(rows().at(-1)?.getAttribute("aria-rowindex")).toBe(String(WINDOW_SIZE));
  });
  await settle();
  expect(window).toHaveBeenCalledTimes(1);
});

test("a refused refetch keeps the rows the list showed", async () => {
  const state = { refused: false };
  const client = new QueryClient();
  renderList({ cache: refusableCache(state), client });
  await screen.findByRole("grid");
  state.refused = true;
  await invalidateWindows(client);
  await settle();
  expect(client.getQueryState(queryKeys.window(FASTMAIL.id, INBOX_ID, 0))?.status).toBe("error");
  expect(rows().length).toBeGreaterThan(0);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("Try again on a refused first page lands the focus on the first row", async () => {
  const state = { refused: true };
  renderList({ cache: refusableCache(state) });
  const button = await screen.findByRole("button", { name: "Try again" });
  button.focus();
  state.refused = false;
  fireEvent.click(button);
  await screen.findByRole("grid");
  await vi.waitFor(() => {
    expect(focused()).toBe(row(0));
  });
});
