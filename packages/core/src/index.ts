// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export { fetchSession, sessionInfoSchema, SignInError, signIn, signOut } from "./session";
export type { SessionInfo, SignInFailureCode } from "./session";
export {
  deviceSchema,
  fetchSessions,
  revokeOtherSessions,
  revokeSession,
  sessionRowSchema,
} from "./sessions";
export type { Device, RevokeOptions, SessionRow } from "./sessions";
