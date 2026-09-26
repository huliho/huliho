// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { AccountRow, ListPage, Mailbox, ThreadDetail } from "@huliho/core";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

import type { Lease } from "../cache/coordinator";
import { ACCOUNTS, EXPIRED, MAILBOXES, MAILBOXES_EMPTY_INBOX, STOPPED } from "./fixtures";
import { mockShellBox, renderShell } from "./shell-rig";

const mailboxes = vi.hoisted(() => vi.fn<(accountId: string) => Promise<Mailbox[]>>());
const page = vi.hoisted(() => vi.fn<() => Promise<ListPage>>());
const attach = vi.hoisted(() => vi.fn<(lease: Lease) => () => void>(() => () => undefined));
const polled = vi.hoisted(() => vi.fn<() => void>());
vi.mock("../cache/client", async () => {
  const fixtures = await import("./fixtures");
  return {
    mailCache: {
      mailboxes,
      window: page,
      thread: vi.fn<() => Promise<ThreadDetail>>(() => Promise.resolve(fixtures.THREAD)),
      reveal: vi.fn<() => Promise<never>>(),
    },
    attachCache: attach,
    clearCache: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    pollCache: polled,
  };
});

const ROWS: AccountRow[] = [...ACCOUNTS, EXPIRED, STOPPED];
const STOPPED_PATH = `/mail/${STOPPED.id}/mb-inbox`;
const STOPPED_SENTENCE =
  "Couldn’t reach the server and stopped trying. The connection is checked again every 15 minutes.";
const STILL_STOPPED_SENTENCE =
  "Still couldn’t reach the server. The connection is checked again every 15 minutes.";
const OFFLINE_SENTENCE = "Offline: showing cached mail. The connection comes back on its own.";

// The account card at the top of the sidebar, by the address it shows.
function card(address: string): HTMLElement {
  return screen.getByRole("button", { name: (name) => name.includes(address) });
}

// What the retry route answers.
function answer(status: number, body: unknown): void {
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>().mockResolvedValue(new Response(JSON.stringify(body), { status })),
  );
}

// The Retry button, focused as a key press would leave it, then pressed.
function pressRetry(): HTMLElement {
  const retry = screen.getByRole("button", { name: "Retry Noordwind" });
  act(() => {
    retry.focus();
  });
  fireEvent.click(retry);
  return retry;
}

mockShellBox();

beforeEach(async () => {
  const { INBOX_PAGE } = await import("./fixtures");
  mailboxes.mockReset();
  mailboxes.mockResolvedValue(MAILBOXES);
  page.mockReset();
  page.mockResolvedValue(INBOX_PAGE);
  polled.mockClear();
});

test("a stopped account shows the banner over the list; a pass removes it, says so and hands the focus to the first row", async () => {
  answer(200, { ...STOPPED, stoppedCause: null, stoppedAt: null });
  renderShell(STOPPED_PATH, { accounts: ROWS });
  expect(await screen.findByText(STOPPED_SENTENCE)).toBeDefined();
  await screen.findByRole("grid", { name: "Conversations" });
  expect(card("sanne@noordwind.nl").textContent).toContain("Stopped");
  pressRetry();
  await vi.waitFor(() => {
    expect(screen.queryByRole("button", { name: /Retry/ })).toBeNull();
  });
  expect(screen.getByText("Connected again.").getAttribute("role")).toBe("status");
  await vi.waitFor(() => {
    expect(document.activeElement?.getAttribute("aria-rowindex")).toBe("1");
  });
  expect(card("sanne@noordwind.nl").textContent).not.toContain("Stopped");
  expect(polled).toHaveBeenCalledOnce();
});

test("a pass while the page was refused hands the focus to Try again; while it loads, to the list", async () => {
  answer(200, { ...STOPPED, stoppedCause: null, stoppedAt: null });
  page.mockRejectedValue(new JmapError("unavailable"));
  renderShell(STOPPED_PATH, { accounts: ROWS });
  await screen.findByText(STOPPED_SENTENCE);
  const tryAgain = await screen.findByRole("button", { name: "Try again" });
  pressRetry();
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(tryAgain);
  });
  cleanup();
  answer(200, { ...STOPPED, stoppedCause: null, stoppedAt: null });
  page.mockReturnValue(new Promise<ListPage>(() => undefined));
  renderShell(STOPPED_PATH, { accounts: ROWS });
  await screen.findByText(STOPPED_SENTENCE);
  const loading = screen.getByRole("status", { name: "Loading…" });
  pressRetry();
  await vi.waitFor(() => {
    expect(document.activeElement?.contains(loading)).toBe(true);
    expect(document.activeElement).not.toBe(document.body);
  });
});

test("an expired account shows Reconnect to the card; offline the strip takes the slot and the banner returns online", async () => {
  renderShell(`/mail/${EXPIRED.id}/mb-inbox`, { accounts: ROWS });
  const sentence = await screen.findByText(
    "The connection to s.bakker@kastanje.studio expired. Mail shown may be out of date.",
  );
  expect(sentence.getAttribute("role")).toBe("status");
  expect(screen.getByRole("link", { name: "Reconnect Kastanje Studio" }).getAttribute("href")).toBe(
    "/accounts/new?reconnect=acc-3",
  );
  expect(card("s.bakker@kastanje.studio").textContent).toContain("Expired");
  const onLine = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  act(() => {
    window.dispatchEvent(new Event("offline"));
  });
  expect(screen.getByText(OFFLINE_SENTENCE)).toBeDefined();
  expect(screen.queryByRole("link", { name: /Reconnect/ })).toBeNull();
  expect(sentence.textContent).toBe("");
  onLine.mockReturnValue(true);
  act(() => {
    window.dispatchEvent(new Event("online"));
  });
  expect(screen.getByRole("link", { name: "Reconnect Kastanje Studio" })).toBeDefined();
});

test("a retry that still fails says so in place and keeps the cursor on the button; a rejected credential turns Retry into Reconnect, which takes the cursor", async () => {
  answer(409, { error: "still_stopped", cause: "connection" });
  renderShell(STOPPED_PATH, { accounts: ROWS });
  const sentence = await screen.findByText(STOPPED_SENTENCE);
  const retry = pressRetry();
  await vi.waitFor(() => {
    expect(sentence.textContent).toBe(STILL_STOPPED_SENTENCE);
  });
  expect(document.activeElement).toBe(retry);
  answer(409, { error: "still_stopped", cause: "credentials" });
  fireEvent.click(retry);
  const reconnect = await screen.findByRole("link", { name: "Reconnect Noordwind" });
  expect(sentence.textContent).toContain("The connection to sanne@noordwind.nl expired.");
  expect(screen.queryByRole("button", { name: /Retry/ })).toBeNull();
  expect(document.activeElement).toBe(reconnect);
});

test("a pass whose focus moved on during the round trip leaves it where it went", async () => {
  const reply = Promise.withResolvers<Response>();
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockReturnValue(reply.promise));
  renderShell(STOPPED_PATH, { accounts: ROWS });
  await screen.findByText(STOPPED_SENTENCE);
  await screen.findByRole("grid", { name: "Conversations" });
  pressRetry();
  const elsewhere = card("sanne@noordwind.nl");
  act(() => {
    elsewhere.focus();
  });
  await act(async () => {
    reply.resolve(
      new Response(JSON.stringify({ ...STOPPED, stoppedCause: null, stoppedAt: null }), {
        status: 200,
      }),
    );
    await reply.promise;
  });
  await vi.waitFor(() => {
    expect(screen.queryByRole("button", { name: /Retry/ })).toBeNull();
  });
  expect(polled).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(elsewhere);
});

test("a retry's answer that lands after a move to another account's mailbox moves nothing there", async () => {
  const reply = Promise.withResolvers<Response>();
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockReturnValue(reply.promise));
  const router = renderShell(STOPPED_PATH, { accounts: ROWS });
  await screen.findByText(STOPPED_SENTENCE);
  pressRetry();
  await act(async () => {
    await router.navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId: "acc-1", mailboxId: "mb-inbox" },
    });
  });
  await screen.findByRole("grid", { name: "Conversations" });
  expect(screen.queryByText(STOPPED_SENTENCE)).toBeNull();
  await act(async () => {
    reply.resolve(
      new Response(JSON.stringify({ ...STOPPED, stoppedCause: null, stoppedAt: null }), {
        status: 200,
      }),
    );
    await reply.promise;
  });
  expect(polled).not.toHaveBeenCalled();
  expect(screen.queryByText("Connected again.")).toBeNull();
  expect(document.activeElement?.getAttribute("aria-rowindex")).toBeNull();
});

test("a retry that settled nothing is named in place, whatever refused it", async () => {
  answer(500, { error: "internal" });
  renderShell(STOPPED_PATH, { accounts: ROWS });
  const sentence = await screen.findByText(STOPPED_SENTENCE);
  fireEvent.click(screen.getByRole("button", { name: "Retry Noordwind" }));
  await vi.waitFor(() => {
    expect(sentence.textContent).toBe("Couldn’t check the connection. Try again in a moment.");
  });
  expect(screen.getByRole("button", { name: "Retry Noordwind" })).toBeDefined();
});

test("an empty mailbox keeps the banner and the offline strip; a pass hands the focus to the empty state", async () => {
  mailboxes.mockResolvedValue(MAILBOXES_EMPTY_INBOX);
  answer(200, { ...STOPPED, stoppedCause: null, stoppedAt: null });
  renderShell(STOPPED_PATH, { accounts: ROWS });
  expect(await screen.findByText("Inbox is empty. Nothing needs you right now.")).toBeDefined();
  expect(screen.getByText(STOPPED_SENTENCE)).toBeDefined();
  const onLine = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  act(() => {
    window.dispatchEvent(new Event("offline"));
  });
  expect(screen.getByText(OFFLINE_SENTENCE).getAttribute("role")).toBe("status");
  onLine.mockReturnValue(true);
  act(() => {
    window.dispatchEvent(new Event("online"));
  });
  fireEvent.click(screen.getByRole("button", { name: "Retry Noordwind" }));
  await vi.waitFor(() => {
    expect(document.activeElement?.textContent).toContain("Inbox is empty.");
  });
});
