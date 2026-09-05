// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export { CredentialError } from "./credentials";
export type { CredentialFailureCode } from "./credentials";
export {
  PASSWORD_MAX_CHARS,
  PASSWORD_MIN_CHARS,
  changePassword,
  fitsPasswordWindow,
} from "./password";
export type { PasswordChangeInput } from "./password";
export { ROLES, grantableRoles, mayManageUsers } from "./role";
export type { Role } from "./role";
export { fetchSession, sessionInfoSchema, signIn, signOut } from "./session";
export type { SessionInfo } from "./session";
export {
  deviceSchema,
  fetchSessions,
  revokeOtherSessions,
  revokeSession,
  sessionRowSchema,
} from "./sessions";
export type { Device, RevokeOptions, SessionRow } from "./sessions";
export {
  USER_NAME_MAX_CHARS,
  UsersError,
  createUser,
  fetchUsers,
  fitsLogin,
  resetPassword,
} from "./users";
export type { CreatedUser, IssuedPassword, NewUser, UserRow, UsersFailureCode } from "./users";
