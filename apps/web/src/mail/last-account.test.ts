// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { beforeEach, expect, test } from "vitest";

import { FASTMAIL, GMAIL } from "./fixtures";
import { forgetLastAccount, landingAccount, rememberLastAccount } from "./last-account";

beforeEach(() => {
  localStorage.clear();
});

test("the root lands in the oldest account until this device opened another", () => {
  expect(landingAccount([GMAIL, FASTMAIL])).toBe("acc-1");
  rememberLastAccount("acc-2");
  expect(landingAccount([GMAIL, FASTMAIL])).toBe("acc-2");
});

test("a remembered account outside the list gives way to the oldest; an empty list gives none", () => {
  rememberLastAccount("acc-9");
  expect(landingAccount([GMAIL, FASTMAIL])).toBe("acc-1");
  expect(landingAccount([])).toBeNull();
});

test("a session that ends forgets the remembered account", () => {
  rememberLastAccount("acc-2");
  forgetLastAccount();
  expect(landingAccount([GMAIL, FASTMAIL])).toBe("acc-1");
});
