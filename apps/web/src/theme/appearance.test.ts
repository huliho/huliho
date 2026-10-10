// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { beforeEach, expect, test } from "vitest";

import {
  DEFAULT_APPEARANCE,
  appearanceOf,
  applyAppearance,
  rememberedAppearance,
} from "./appearance";

const ATTRIBUTES = ["data-theme", "data-density", "data-font-size", "data-line-height"];

beforeEach(() => {
  localStorage.clear();
  for (const attribute of ATTRIBUTES) {
    document.documentElement.removeAttribute(attribute);
  }
});

test("applying sets the attributes the stylesheet reads and remembers them", () => {
  const chosen = {
    theme: "dark",
    density: "compact",
    fontSize: "larger",
    lineHeight: "loose",
  } as const;
  applyAppearance(document, chosen);
  expect(document.documentElement.dataset["theme"]).toBe("dark");
  expect(document.documentElement.dataset["density"]).toBe("compact");
  expect(document.documentElement.dataset["fontSize"]).toBe("larger");
  expect(document.documentElement.dataset["lineHeight"]).toBe("loose");
  expect(rememberedAppearance()).toEqual(chosen);
});

test("a device without a memory, or with a word off the list, starts at the default", () => {
  expect(rememberedAppearance()).toEqual(DEFAULT_APPEARANCE);
  localStorage.setItem("huliho-theme", "sepia");
  localStorage.setItem("huliho-density", "compact");
  localStorage.setItem("huliho-font-size", "huge");
  localStorage.setItem("huliho-line-height", "relaxed");
  expect(rememberedAppearance()).toEqual({
    theme: "system",
    density: "compact",
    fontSize: "default",
    lineHeight: "relaxed",
  });
});

test("the server's words name an appearance, a key never chosen at its default", () => {
  expect(appearanceOf({})).toEqual(DEFAULT_APPEARANCE);
  expect(appearanceOf({ theme: "light", fontSize: "large", readingPane: "off" })).toEqual({
    theme: "light",
    density: "comfortable",
    fontSize: "large",
    lineHeight: "default",
  });
});
