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
export {
  actionFor,
  actionMutations,
  moveMutations,
  offeredActions,
  offeredJobs,
  verbOf,
} from "./actions";
export type { ActionTarget, ChangeVerb, MailAction, Offered } from "./actions";
export { fitsAddress, fitsHostName } from "./address";
export type { Authentication, AuthenticationResults, AuthVerdict } from "./auth-results";
export type { BodyDetail, MailCache, ThreadDetail, Undo } from "./cache/api";
export { readBody } from "./cache/bodies";
export { flushPending } from "./cache/flush";
export type { Flushed } from "./cache/flush";
export { jobRound, jobStarted, runJob } from "./cache/jobs";
export type { Job, JobControls, JobProgress, JobRound, JobState } from "./cache/jobs";
export {
  CHANGES_ROUNDS_MAX,
  PREVIEW_BATCH,
  SEARCH_ROWS,
  SENDER_SUGGESTIONS,
  WINDOW_SIZE,
} from "./cache/limits";
export { syncMailboxes } from "./cache/mailboxes";
export { MemoryMailStore } from "./cache/memory";
export { dropHeld, mutateMany, releaseHeld, releaseLapsed } from "./cache/pending";
export type { DestroyMutation, Logged, Mutation, PatchMutation } from "./cache/pending";
export { applyChanges } from "./cache/poll";
export type { AppliedChanges } from "./cache/poll";
export { heldQuotaSchema, readQuota } from "./cache/quota";
export type { HeldQuota, Quota, QuotaReading } from "./cache/quota";
export { revealNewMail } from "./cache/refresh";
export { mayPatch } from "./cache/rights";
export { firstSyncOf, listPage } from "./cache/rows";
export type { FirstSync, ListPage, ListRow } from "./cache/rows";
export {
  EVERY_MAILBOX,
  readSearchParams,
  scopeOf,
  searchFilterOf,
  searchKeyOf,
  searchParamsOf,
} from "./cache/search";
export type { Scope } from "./cache/scope";
export type { SearchParams } from "./cache/search";
export { parseQuery, queryOf } from "./cache/search-text";
export type { QueryContext } from "./cache/search-text";
export { searchWindow } from "./cache/search-window";
export type { SearchAsk } from "./cache/search-window";
export { readSenders, sendersOf } from "./cache/senders";
export { treeOrder } from "./cache/tree-order";
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
  SearchRow,
  Sender,
  StoreArea,
  ThreadRow,
} from "./cache/store";
export { readThread } from "./cache/thread";
export { queryWindow } from "./cache/window";
export type { WindowPage } from "./cache/window";
export { CredentialError } from "./credentials";
export type { CredentialFailureCode } from "./credentials";
export { downloadUrl } from "./download";
export type { BlobRequest, DownloadSource } from "./download";
export { BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE } from "./jmap/body";
export type { EmailBodyPart, EmailBodyValue } from "./jmap/body";
export type { EmailFilter, ObjectType } from "./jmap/calls";
export { JmapClient, JmapError, MethodFailure } from "./jmap/client";
export type { JmapFailureCode, JmapSession } from "./jmap/client";
export { HULIHO_CAPABILITY, emailHeaderSchema, mailboxSchema } from "./jmap/schemas";
export type { EmailHeader, EmailState, Mailbox, Thread } from "./jmap/schemas";
export { linkRisk, linkText } from "./links";
export type { LinkRisk } from "./links";
export {
  PASSWORD_MAX_CHARS,
  PASSWORD_MIN_CHARS,
  changePassword,
  fitsPasswordWindow,
} from "./password";
export type { PasswordChangeInput } from "./password";
export {
  DENSITIES,
  FONT_SIZES,
  LINE_HEIGHTS,
  PreferencesError,
  THEMES,
  fetchPreferences,
  isPreferenceLocale,
  setPreference,
  withPreference,
} from "./preferences";
export type {
  AfterAction,
  DarkMail,
  Density,
  FontSize,
  LineHeight,
  PreferenceChange,
  PreferenceLocale,
  Preferences,
  ReadingPane,
  Theme,
} from "./preferences";
export { classifyImageUrl, classifyLinkUrl, remoteImageUrl } from "./remote";
export type { ImageSource, LinkTarget } from "./remote";
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
export { quotedLines, unflow } from "./text/flowed";
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
