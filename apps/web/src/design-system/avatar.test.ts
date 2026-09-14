// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { initialsOf } from "./avatar";

test("initials take the first letter of the first two words, uppercased", () => {
  expect(initialsOf("Fastmail", "en")).toBe("F");
  expect(initialsOf("Sanne Bakker", "en")).toBe("SB");
  expect(initialsOf("dekker-mail.nl", "en")).toBe("D");
  expect(initialsOf("  élan  vital ", "en")).toBe("ÉV");
  expect(initialsOf("Ada Lovelace Byron", "en")).toBe("AL");
  expect(initialsOf("", "en")).toBe("");
});
