// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export {
  AccountsError,
  addAccount,
  discoverServer,
  endConsent,
  fetchAccounts,
  fetchConsent,
  removeAccount,
  replaceCredential,
  retryAccount,
  startConsent,
} from "./accounts";
export type {
  AccountList,
  AccountRow,
  AccountTarget,
  AccountsFailureCode,
  AuthMethod,
  ConsentDeniedCause,
  ConsentInput,
  ConsentOutcome,
  Credential,
  CredentialKind,
  FoundServer,
  NewAccountInput,
  Provider,
  RemoveOptions,
  RetryResult,
  StartedConsent,
  StopCause,
  TlsMode,
} from "./accounts";
export { fitsAddress, fitsHostName } from "./address";
export type { Authentication, AuthenticationResults, AuthVerdict } from "./auth-results";
export type { MailCache, ThreadDetail } from "./cache/api";
export { readBody } from "./cache/bodies";
export { CHANGES_ROUNDS_MAX, PREVIEW_BATCH, WINDOW_SIZE } from "./cache/limits";
export { syncMailboxes } from "./cache/mailboxes";
export { MemoryMailStore } from "./cache/memory";
export { applyPatch, flushPending } from "./cache/pending";
export type { Flushed, Mutation } from "./cache/pending";
export { applyChanges } from "./cache/poll";
export type { AppliedChanges } from "./cache/poll";
export { revealNewMail } from "./cache/refresh";
export { firstSyncOf, listPage } from "./cache/rows";
export type { FirstSync, ListPage, ListRow } from "./cache/rows";
export {
  emailBodySchema,
  pendingRowSchema,
  queryRowSchema,
  threadRowSchema,
} from "./cache/schemas";
export type {
  Batch,
  BodySize,
  EmailBody,
  EmailPatch,
  FreshPage,
  MailStore,
  MemberState,
  Page,
  PendingRow,
  QueryRow,
  StoreArea,
  ThreadRow,
} from "./cache/store";
export { readThread } from "./cache/thread";
export { queryWindow } from "./cache/window";
export type { WindowPage } from "./cache/window";
export { CredentialError } from "./credentials";
export type { CredentialFailureCode } from "./credentials";
export type { EmailBodyPart, EmailBodyValue } from "./jmap/body";
export type { ObjectType } from "./jmap/calls";
export { JmapClient, JmapError, MethodFailure } from "./jmap/client";
export type { JmapFailureCode, JmapSession } from "./jmap/client";
export { HULIHO_CAPABILITY, emailHeaderSchema, mailboxSchema } from "./jmap/schemas";
export type { EmailHeader, EmailState, Mailbox, Thread } from "./jmap/schemas";
export {
  PASSWORD_MAX_CHARS,
  PASSWORD_MIN_CHARS,
  changePassword,
  fitsPasswordWindow,
} from "./password";
export type { PasswordChangeInput } from "./password";
export {
  DENSITIES,
  PreferencesError,
  THEMES,
  fetchPreferences,
  isPreferenceLocale,
  setPreference,
  withPreference,
} from "./preferences";
export type {
  Density,
  PreferenceChange,
  PreferenceLocale,
  Preferences,
  ReadingPane,
  Theme,
} from "./preferences";
export { classifyImageUrl, remoteImageUrl } from "./remote";
export type { ImageSource } from "./remote";
export { ROLES, grantableRoles, mayManageUsers } from "./role";
export type { Role } from "./role";
export {
  SenderPoliciesError,
  allowRemoteContent,
  blockRemoteContent,
  fetchSenderPolicies,
  grantFor,
  grantLoads,
  senderKey,
} from "./sender-policies";
export type {
  RemoteContentGrant,
  SenderPoliciesFailureCode,
  SenderPolicy,
} from "./sender-policies";
export { fetchSession, sessionInfoSchema, signIn, signOut } from "./session";
export type { SessionInfo, SignInProvider } from "./session";
export {
  deviceSchema,
  fetchSessions,
  revokeOtherSessions,
  revokeSession,
  sessionRowSchema,
} from "./sessions";
export type { Device, RevokeOptions, SessionRow } from "./sessions";
export { unflow } from "./text/flowed";
export type { QuotedLine } from "./text/flowed";
export {
  USER_NAME_MAX_CHARS,
  UsersError,
  createUser,
  fetchUsers,
  fitsLogin,
  resetPassword,
} from "./users";
export type { CreatedUser, IssuedPassword, NewUser, UserRow, UsersFailureCode } from "./users";
