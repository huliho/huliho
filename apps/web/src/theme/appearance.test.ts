// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { beforeEach, expect, test } from "vitest";

import { DEFAULT_APPEARANCE, applyAppearance, rememberedAppearance } from "./appearance";

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset["theme"];
  delete document.documentElement.dataset["density"];
});

test("applying sets the attributes the stylesheet reads and remembers them", () => {
  applyAppearance(document, { theme: "dark", density: "compact" });
  expect(document.documentElement.dataset["theme"]).toBe("dark");
  expect(document.documentElement.dataset["density"]).toBe("compact");
  expect(rememberedAppearance()).toEqual({ theme: "dark", density: "compact" });
});

test("a device without a memory, or with a word off the list, starts at the default", () => {
  expect(rememberedAppearance()).toEqual(DEFAULT_APPEARANCE);
  localStorage.setItem("huliho-theme", "sepia");
  localStorage.setItem("huliho-density", "compact");
  expect(rememberedAppearance()).toEqual({ theme: "system", density: "compact" });
});
