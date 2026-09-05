// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "./schema";

// Lowest first; a role may grant any role up to its own.
export const ROLES = ["member", "admin", "owner"] as const;

export const roleSchema = z.enum(ROLES);

export type Role = z.infer<typeof roleSchema>;

export function grantableRoles(actor: Role): Role[] {
  return ROLES.slice(0, ROLES.indexOf(actor) + 1);
}

export function mayManageUsers(role: Role): boolean {
  return role !== "member";
}
