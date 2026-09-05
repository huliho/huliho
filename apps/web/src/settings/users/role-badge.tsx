// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Role } from "@huliho/core";

import { Badge } from "../../design-system/badge";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

export function roleLabel(role: Role, locale: Locale): string {
  if (role === "owner") {
    return m.role_owner({}, { locale });
  }
  return role === "admin" ? m.role_admin({}, { locale }) : m.role_member({}, { locale });
}

// Owners and admins stand out; a member is the plain case.
export function RoleBadge({ role, locale }: { role: Role; locale: Locale }) {
  return <Badge tone={role === "member" ? "neutral" : "accent"}>{roleLabel(role, locale)}</Badge>;
}
