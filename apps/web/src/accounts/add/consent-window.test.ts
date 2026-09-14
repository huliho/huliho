// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { openConsentWindow } from "./consent-window";

const URL = "https://accounts.google.com/o/oauth2/v2/auth?state=s1";

interface FakeWindow {
  opener: unknown;
  location: { assign: Mock<(url: string) => void> };
}

function fakeWindow(): FakeWindow {
  const handle: FakeWindow = {
    opener: window,
    location: {
      assign: vi.fn<(url: string) => void>(() => {
        // The provider must never find an opener: it is cut before the navigation.
        expect(handle.opener).toBeNull();
      }),
    },
  };
  return handle;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the window opens blank from the click, loses its opener and then goes to the provider", () => {
  const handle = fakeWindow();
  const open = vi.fn<() => FakeWindow>(() => handle);
  vi.stubGlobal("open", open);
  expect(openConsentWindow(null)).toBe(handle);
  expect(open).toHaveBeenCalledWith("", "_blank", expect.stringContaining("popup"));
  expect(handle.opener).toBeNull();
  expect(handle.location.assign).not.toHaveBeenCalled();
  expect(openConsentWindow(URL)).toBe(handle);
  expect(handle.location.assign).toHaveBeenCalledExactlyOnceWith(URL);
});

test("a browser that keeps the window closed answers null", () => {
  vi.stubGlobal(
    "open",
    vi.fn<() => null>(() => null),
  );
  expect(openConsentWindow(null)).toBeNull();
  expect(openConsentWindow(URL)).toBeNull();
});
