// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";

import { setLocale } from "../paraglide/runtime.js";
import { listedLocales, preferredLocale, switchLocale, useLocale } from "./locale";

function Shows() {
  return <output>{useLocale()}</output>;
}

beforeEach(async () => {
  localStorage.clear();
  await setLocale("en", { reload: false });
});

afterEach(cleanup);

test("a switch re-renders every subscriber, sets the document language and sticks", () => {
  render(
    <>
      <Shows />
      <Shows />
    </>,
  );
  expect(screen.getAllByRole("status").map((node) => node.textContent)).toEqual(["en", "en"]);
  act(() => {
    switchLocale("nl");
  });
  expect(screen.getAllByRole("status").map((node) => node.textContent)).toEqual(["nl", "nl"]);
  expect(document.documentElement.lang).toBe("nl");
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("nl");
});

test("the browser's first language among ours wins, else the base locale", () => {
  Object.defineProperty(navigator, "languages", { value: ["de-DE", "nl-BE"], configurable: true });
  expect(preferredLocale()).toBe("nl");
  Object.defineProperty(navigator, "languages", { value: ["de-DE"], configurable: true });
  expect(preferredLocale()).toBe("en");
  Object.defineProperty(navigator, "languages", { value: ["en-XA"], configurable: true });
  expect(preferredLocale()).toBe("en");
});

test("the listed locales keep the current one and, in development, the pseudo locale", () => {
  expect(listedLocales("en")).toEqual(["en", "nl", "en-XA"]);
  expect(listedLocales("en-XA")).toContain("en-XA");
});
