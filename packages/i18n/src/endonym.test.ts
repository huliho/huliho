// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { localeEndonym } from "./endonym.js";

test("a locale is named in its own language", () => {
  expect(localeEndonym("en")).toBe("English");
  expect(localeEndonym("nl")).toBe("Nederlands");
});

test("the pseudo locale tag resolves to a label", () => {
  expect(localeEndonym("en-XA")).not.toBe("");
});
