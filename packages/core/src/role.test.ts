// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { grantableRoles, mayManageUsers } from "./role";

test("a role grants any role up to its own, lowest first", () => {
  expect(grantableRoles("member")).toEqual(["member"]);
  expect(grantableRoles("admin")).toEqual(["member", "admin"]);
  expect(grantableRoles("owner")).toEqual(["member", "admin", "owner"]);
});

test("admins and owners manage users, members do not", () => {
  expect(mayManageUsers("member")).toBe(false);
  expect(mayManageUsers("admin")).toBe(true);
  expect(mayManageUsers("owner")).toBe(true);
});
