// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";

import type { RetryOutcome } from "../accounts/use-retry-account";
import { EXPIRED, PROBE_INTERVAL_MINUTES, STOPPED } from "./fixtures";
import { routed } from "./story-router";
import { ThreadListBanner } from "./thread-list-banner";

const STOPPED_SENTENCE =
  "Couldn’t reach the server and stopped trying. The connection is checked again every 15 minutes.";
const RESUMED: AccountRow = { ...STOPPED, stoppedCause: null, stoppedAt: null };

// The banner of the stopped account with a button that lets the retry pass.
function Passing() {
  const [passed, setPassed] = useState(false);
  return (
    <>
      <ThreadListBanner
        locale="en"
        account={passed ? RESUMED : STOPPED}
        probeIntervalMinutes={PROBE_INTERVAL_MINUTES}
        online
        outcome={passed ? "resumed" : undefined}
        onRetry={() => {
          setPassed(true);
        }}
        takeFocus={false}
        onFocusTaken={nothing}
      />
      <button
        type="button"
        onClick={() => {
          setPassed(true);
        }}
      >
        pass
      </button>
    </>
  );
}

interface Options {
  online?: boolean;
  outcome?: RetryOutcome;
  onRetry?: () => void;
  takeFocus?: boolean;
  onFocusTaken?: () => void;
}

function nothing(): void {
  // The test renders states; nothing runs.
}

// The banner on the router its link needs; the router draws on the next tick.
async function renderBanner(account: AccountRow, options: Options = {}): Promise<void> {
  render(
    routed(() => (
      <ThreadListBanner
        locale="en"
        account={account}
        probeIntervalMinutes={PROBE_INTERVAL_MINUTES}
        online={options.online ?? true}
        outcome={options.outcome}
        onRetry={options.onRetry ?? vi.fn<() => void>()}
        takeFocus={options.takeFocus ?? false}
        onFocusTaken={options.onFocusTaken ?? nothing}
      />
    )),
  );
  await screen.findByRole("status");
}

// The one status region of the banner.
function status(): HTMLElement {
  return screen.getByRole("status");
}

function banner(): HTMLElement {
  const box = status().parentElement;
  if (box === null) {
    throw new Error("the region has no banner around it");
  }
  return box;
}

afterEach(cleanup);

test("an expired account gets the danger cause, its sentence and Reconnect as a link to the card", async () => {
  await renderBanner(EXPIRED);
  await vi.waitFor(() => {
    expect(status().textContent).toBe(
      "The connection to s.bakker@kastanje.studio expired. Mail shown may be out of date.",
    );
  });
  expect(banner().dataset["cause"]).toBe("expired");
  const link = screen.getByRole("link", { name: "Reconnect Kastanje Studio" });
  expect(link.getAttribute("href")).toBe("/accounts/new?reconnect=acc-3");
  expect(link.textContent).toBe("Reconnect");
  expect(screen.queryByRole("button")).toBeNull();
});

test("a stopped account gets the warn cause, the interval sentence and Retry", async () => {
  const onRetry = vi.fn<() => void>();
  await renderBanner(STOPPED, { onRetry });
  await vi.waitFor(() => {
    expect(status().textContent).toBe(STOPPED_SENTENCE);
  });
  expect(banner().dataset["cause"]).toBe("stopped");
  const retry = screen.getByRole("button", { name: "Retry Noordwind" });
  expect(retry.textContent).toBe("Retry");
  fireEvent.click(retry);
  expect(onRetry).toHaveBeenCalledOnce();
  expect(screen.queryByRole("link")).toBeNull();
});

test("the outcome of a retry takes the sentence's place while the stop stands", async () => {
  await renderBanner(STOPPED, { outcome: "pending" });
  const retrying = screen.getByRole("button", { name: "Retrying Noordwind…" });
  expect(retrying.getAttribute("aria-busy")).toBe("true");
  await vi.waitFor(() => {
    expect(status().textContent).toBe(STOPPED_SENTENCE);
  });
  cleanup();
  await renderBanner(STOPPED, { outcome: "stillStopped" });
  await vi.waitFor(() => {
    expect(status().textContent).toBe(
      "Still couldn’t reach the server. The connection is checked again every 15 minutes.",
    );
  });
  cleanup();
  await renderBanner(STOPPED, { outcome: "failed" });
  await vi.waitFor(() => {
    expect(status().textContent).toBe("Couldn’t check the connection. Try again in a moment.");
  });
  expect(screen.getByRole("button", { name: "Retry Noordwind" })).toBeDefined();
});

test("a pass takes the action away at once and keeps the banner drawn while it fades, its word in the region", async () => {
  render(routed(() => <Passing />));
  await screen.findByRole("status");
  await vi.waitFor(() => {
    expect(status().textContent).toBe(STOPPED_SENTENCE);
  });
  fireEvent.click(screen.getByRole("button", { name: "Retry Noordwind" }));
  expect(screen.queryByRole("button", { name: /Retry/ })).toBeNull();
  expect(banner().hasAttribute("data-leaving")).toBe(true);
  expect(banner().dataset["cause"]).toBe("stopped");
  expect(status().textContent).toBe("Connected again.");
  expect(screen.getByText(STOPPED_SENTENCE).getAttribute("aria-hidden")).toBe("true");
  await vi.waitFor(() => {
    expect(banner().hasAttribute("data-idle")).toBe(true);
  });
  expect(banner().hasAttribute("data-leaving")).toBe(false);
  expect(screen.queryByText(STOPPED_SENTENCE)).toBeNull();
});

test("offline the banner is idle with its region kept empty; a pass leaves its word behind while the account runs", async () => {
  await renderBanner(STOPPED, { online: false });
  expect(banner().hasAttribute("data-idle")).toBe(true);
  expect(banner().dataset["cause"]).toBeUndefined();
  expect(status().textContent).toBe("");
  expect(screen.queryByRole("button")).toBeNull();
  cleanup();
  await renderBanner(RESUMED, { outcome: "resumed" });
  expect(banner().hasAttribute("data-idle")).toBe(true);
  await vi.waitFor(() => {
    expect(status().textContent).toBe("Connected again.");
  });
  cleanup();
  // A pass long ago, then a stop the poll found, then offline: nothing spoken.
  await renderBanner(STOPPED, { online: false, outcome: "resumed" });
  expect(status().textContent).toBe("");
});

test("a banner that mounts expired after a retry took no focus with it leaves the focus alone", async () => {
  await renderBanner(EXPIRED, { outcome: "stillStopped" });
  expect(screen.getByRole("link", { name: "Reconnect Kastanje Studio" })).toBeDefined();
  expect(document.activeElement).toBe(document.body);
});

test("a retry that turned the stop into an expired one hands the focus to Reconnect once and says so", async () => {
  const onFocusTaken = vi.fn<() => void>();
  await renderBanner(EXPIRED, { outcome: "stillStopped", takeFocus: true, onFocusTaken });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("link", { name: /Reconnect/ }));
  });
  expect(onFocusTaken).toHaveBeenCalledOnce();
  cleanup();
  // Offline the link is not drawn; the ask is spent all the same.
  await renderBanner(EXPIRED, {
    online: false,
    outcome: "stillStopped",
    takeFocus: true,
    onFocusTaken,
  });
  expect(document.activeElement).toBe(document.body);
  expect(onFocusTaken).toHaveBeenCalledTimes(2);
});
