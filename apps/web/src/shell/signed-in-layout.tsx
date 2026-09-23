// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Outlet } from "@tanstack/react-router";

import { useAppliedPreferences } from "../theme/use-applied-preferences";

// Wraps every route a guard admits with a session. It mounts once the
// guard has read that session and leaves with it, so a sign-in on the
// same page starts it afresh with the fresh session.
export function SignedInLayout() {
  useAppliedPreferences();
  return <Outlet />;
}
