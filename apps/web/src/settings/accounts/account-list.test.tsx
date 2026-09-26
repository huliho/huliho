// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import type { RetryOutcomes } from "../../accounts/use-retry-account";
import { AccountList } from "./account-list";

const NOW = 1_778_750_400_000;
const MINUTES = 15;

const CONNECTED: AccountRow = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: NOW,
};
const EXPIRED: AccountRow = {
  ...CONNECTED,
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "oauth2",
  stoppedCause: "credentials",
  stoppedAt: NOW,
};
const STOPPED: AccountRow = {
  ...CONNECTED,
  id: "acc-3",
  address: "sanne@dekker-mail.nl",
  name: "dekker-mail.nl",
  provider: "generic",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "connection",
  stoppedAt: NOW,
};
const ROWS = [CONNECTED, EXPIRED, STOPPED];

interface Rendered {
  onRetry: Mock<(id: string) => void>;
  onRemove: Mock<(id: string) => void>;
}

interface ScreenProps extends Rendered {
  rows: AccountRow[];
  outcomes: RetryOutcomes;
}

// The page's shape around the list: the link the last removal hands the cursor to.
function Screen({ rows, outcomes, onRetry, onRemove }: ScreenProps) {
  const add = useRef<HTMLAnchorElement>(null);
  return (
    <>
      <AccountList
        rows={rows}
        locale="en"
        probeIntervalMinutes={MINUTES}
        outcomes={outcomes}
        onRetry={onRetry}
        onRemove={onRemove}
        afterLast={add}
      />
      <a ref={add} href="/accounts/new">
        Add account
      </a>
    </>
  );
}

// The Reconnect link needs a router; a memory one at the page's address serves.
async function renderList(rows: AccountRow[], outcomes: RetryOutcomes = {}): Promise<Rendered> {
  const rendered: Rendered = {
    onRetry: vi.fn<(id: string) => void>(),
    onRemove: vi.fn<(id: string) => void>(),
  };
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => <Screen rows={rows} outcomes={outcomes} {...rendered} />,
    }),
    history: createMemoryHistory({ initialEntries: ["/settings/accounts"] }),
  });
  render(<RouterProvider router={router} />);
  await screen.findByRole("list");
  return rendered;
}

afterEach(cleanup);

test("a connected row offers Remove alone; a stop offers Reconnect or Retry with its sentence", async () => {
  await renderList(ROWS);
  const items = screen.getAllByRole("listitem");
  expect(items).toHaveLength(3);
  expect(items[0]?.textContent).toContain("Fastmail");
  expect(items[0]?.textContent).toContain("sanne@fastmail.com");
  expect(items[0]?.querySelector('[aria-hidden="true"]')?.textContent).toBe("F");
  expect(screen.getByRole("button", { name: "Remove Fastmail" })).toBeDefined();
  expect(screen.queryByRole("button", { name: "Retry Fastmail" })).toBeNull();
  expect(items[1]?.textContent).toContain("Connection expired");
  expect(screen.getByRole("link", { name: "Reconnect Gmail" }).getAttribute("href")).toBe(
    "/accounts/new?reconnect=acc-2",
  );
  expect(items[2]?.textContent).toContain("stopped trying");
  expect(items[2]?.textContent).toContain("every 15 minutes");
  expect(screen.getByRole("button", { name: "Retry dekker-mail.nl" })).toBeDefined();
  expect(screen.getAllByRole("button", { name: /^Remove/ })).toHaveLength(3);
});

test("Retry and Remove name their row; a pending retry is held and says so", async () => {
  const rendered = await renderList(ROWS);
  fireEvent.click(screen.getByRole("button", { name: "Retry dekker-mail.nl" }));
  expect(rendered.onRetry).toHaveBeenCalledExactlyOnceWith("acc-3");
  fireEvent.click(screen.getByRole("button", { name: "Remove Gmail" }));
  expect(rendered.onRemove).toHaveBeenCalledExactlyOnceWith("acc-2");
  cleanup();
  await renderList(ROWS, { "acc-3": "pending" });
  const retrying = screen.getByRole("button", { name: "Retrying dekker-mail.nl…" });
  expect(retrying.textContent).toBe("Retrying…");
  expect(retrying.getAttribute("aria-busy")).toBe("true");
});

test("a settled retry reads as a status or an alert on its row", async () => {
  await renderList(ROWS, { "acc-1": "resumed", "acc-3": "stillStopped" });
  expect(screen.getByRole("status").textContent).toBe("Connected again.");
  expect(screen.getByRole("alert").textContent).toContain("Still couldn’t reach the server");
  cleanup();
  await renderList(ROWS, { "acc-3": "failed" });
  expect(screen.getByRole("alert").textContent).toContain("Couldn’t check the connection");
});

test("Remove hands the cursor to the next row's Remove, then back, then to the link", async () => {
  await renderList(ROWS);
  const gmail = screen.getByRole("button", { name: "Remove Gmail" });
  gmail.focus();
  fireEvent.click(gmail);
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Remove dekker-mail.nl" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Remove dekker-mail.nl" }));
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Remove Fastmail" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove Fastmail" }));
  expect(document.activeElement).toBe(screen.getByRole("link", { name: "Add account" }));
});
