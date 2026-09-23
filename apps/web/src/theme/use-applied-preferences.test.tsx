// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Preferences } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { useLocale } from "../i18n/locale";
import { setLocale } from "../paraglide/runtime.js";
import { useAppliedPreferences } from "./use-applied-preferences";

function Harness() {
  useAppliedPreferences();
  return <output>{useLocale()}</output>;
}

function answer(preferences: Preferences): Mock<typeof fetch> {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(new Response(JSON.stringify(preferences), { status: 200 }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

async function renderApplied(fetchMock: Mock<typeof fetch>): Promise<void> {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Harness />
    </QueryClientProvider>,
  );
  await vi.waitFor(() => {
    expect(fetchMock).toHaveBeenCalled();
  });
  await vi.waitFor(() => {
    expect(document.documentElement.dataset["theme"]).toBeDefined();
  });
}

beforeEach(async () => {
  localStorage.clear();
  delete document.documentElement.dataset["theme"];
  delete document.documentElement.dataset["density"];
  Object.defineProperty(navigator, "languages", { value: ["nl-NL", "en"], configurable: true });
  await setLocale("en", { reload: false });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("the server's words land on the document and on the screen", async () => {
  const fetchMock = answer({ theme: "dark", density: "compact", locale: "nl" });
  await renderApplied(fetchMock);
  expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/preferences");
  expect(document.documentElement.dataset["theme"]).toBe("dark");
  expect(document.documentElement.dataset["density"]).toBe("compact");
  expect(document.documentElement.lang).toBe("nl");
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("nl");
});

test("a key never chosen applies its default, the browser's language included", async () => {
  localStorage.setItem("PARAGLIDE_LOCALE", "en");
  await renderApplied(answer({}));
  expect(document.documentElement.dataset["theme"]).toBe("system");
  expect(document.documentElement.dataset["density"]).toBe("comfortable");
  expect(document.documentElement.lang).toBe("nl");
});

test("the pseudo locale stays whatever the server holds", async () => {
  await setLocale("en-XA", { reload: false });
  await renderApplied(answer({ locale: "nl" }));
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("en-XA");
});
