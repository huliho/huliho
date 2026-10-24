// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, QuotaReading } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";

import type { Locale } from "../paraglide/runtime.js";
import { fixtureCache } from "./fixture-cache";
import { prefetchMenus } from "./menus-chunk";
import { QuotaFoot } from "./quota-foot";

const ACCOUNT_ID = "acc-1";
const GB = 1000 ** 3;
// Long enough for the test runner to transform the menus chunk once.
const CHUNK_TIMEOUT_MS = 15_000;
// Base UI waits this long under a resting pointer before a hint opens.
const HOVER_DELAY_MS = 600;
const READING: QuotaReading = {
  unit: "octets",
  used: 4.2 * GB,
  limit: 10 * GB,
  share: 0.42,
  warn: false,
};

function answering(quota: Promise<QuotaReading | null>): MailCache {
  return { ...fixtureCache({}), quota: () => quota };
}

function renderFoot(cache: MailCache, variant: "foot" | "rail" = "foot", locale: Locale = "en") {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <QuotaFoot locale={locale} cache={cache} accountId={ACCOUNT_ID} variant={variant} />
    </QueryClientProvider>,
  );
  return client;
}

beforeAll(async () => {
  await prefetchMenus();
}, CHUNK_TIMEOUT_MS);

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("the account's quota stands as a meter that is a tab stop, its hint the whole sentence", async () => {
  renderFoot(answering(Promise.resolve(READING)));
  const meter = await screen.findByRole("meter", { name: "Mail storage: 4.2 GB of 10 GB used" });
  expect(meter.textContent).toContain("4.2 of 10 GB");
  expect(meter.tabIndex).toBe(0);
  vi.useFakeTimers();
  fireEvent.pointerEnter(meter, { pointerType: "mouse" });
  fireEvent.mouseEnter(meter);
  fireEvent.mouseMove(meter);
  act(() => {
    vi.advanceTimersByTime(HOVER_DELAY_MS);
  });
  expect(screen.getByText("Mail storage: 4.2 GB of 10 GB used")).toBeDefined();
});

test("the rail draws the bar alone, named in the reader's language", async () => {
  renderFoot(answering(Promise.resolve(READING)), "rail", "nl");
  const meter = await screen.findByRole("meter", { name: "Mailopslag: 4,2 GB van 10 GB gebruikt" });
  expect(meter.textContent).not.toContain("van 10 GB");
});

test("no quota, a read on its way and a refused read show nothing and say nothing", async () => {
  const none = renderFoot(answering(Promise.resolve(null)));
  renderFoot(answering(new Promise(() => undefined)));
  const refused = renderFoot(answering(Promise.reject(new Error("refused"))));
  await waitFor(() => {
    expect(none.getQueryState(queryKeys.quota(ACCOUNT_ID))?.status).toBe("success");
    expect(refused.getQueryState(queryKeys.quota(ACCOUNT_ID))?.status).toBe("error");
  });
  expect(screen.queryByRole("meter")).toBeNull();
  expect(document.body.textContent).toBe("");
});
