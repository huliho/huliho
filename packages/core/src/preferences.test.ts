// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import {
  PreferencesError,
  fetchPreferences,
  isPreferenceLocale,
  setPreference,
  withPreference,
} from "./preferences";

function answer(status: number, body?: unknown): Mock<typeof fetch> {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(new Response(body === undefined ? null : JSON.stringify(body), { status }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the answer parses with absent keys and refuses a word off a list", async () => {
  answer(200, {});
  expect(await fetchPreferences()).toEqual({});
  answer(200, { theme: "dark", locale: "nl" });
  expect(await fetchPreferences()).toEqual({ theme: "dark", locale: "nl" });
  answer(200, { readingPane: "left" });
  await expect(fetchPreferences()).rejects.toThrow(/invalid/i);
  answer(200, { locale: "en-XA" });
  await expect(fetchPreferences()).rejects.toThrow(/invalid/i);
  answer(500, { error: "internal" });
  await expect(fetchPreferences()).rejects.toThrow("preferences request failed");
});

test("a write names the key in the path and carries the word with the CSRF header", async () => {
  const fetchMock = answer(204);
  await setPreference({ key: "readingPane", value: "bottom" });
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/preferences/readingPane");
  expect(init?.method).toBe("PUT");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(init?.body).toBe(JSON.stringify({ value: "bottom" }));
});

test("a session that ended is named; every other refusal reads as unavailable", async () => {
  answer(401, { error: "unauthenticated" });
  await expect(setPreference({ key: "theme", value: "dark" })).rejects.toMatchObject({
    name: "PreferencesError",
    code: "unauthenticated",
  });
  answer(400, { error: "invalid_request" });
  await expect(setPreference({ key: "theme", value: "dark" })).rejects.toMatchObject({
    code: "unavailable",
  });
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockRejectedValue(new TypeError("offline")));
  const failure = await setPreference({ key: "density", value: "compact" }).catch(
    (error: unknown) => error,
  );
  expect(failure).toBeInstanceOf(PreferencesError);
  expect(failure).toMatchObject({ code: "unavailable" });
});

test("a change replaces one key and leaves the rest", () => {
  const current = { theme: "dark", density: "compact" } as const;
  expect(withPreference(current, { key: "theme", value: "light" })).toEqual({
    theme: "light",
    density: "compact",
  });
  expect(withPreference(current, { key: "locale", value: "nl" })).toEqual({
    theme: "dark",
    density: "compact",
    locale: "nl",
  });
  expect(withPreference({}, { key: "readingPane", value: "off" })).toEqual({ readingPane: "off" });
  expect(withPreference({}, { key: "density", value: "comfortable" })).toEqual({
    density: "comfortable",
  });
});

test("only the words the server stores count as a locale preference", () => {
  expect(isPreferenceLocale("nl")).toBe(true);
  expect(isPreferenceLocale("en-XA")).toBe(false);
});
