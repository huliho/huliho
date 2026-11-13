// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// What the server answers for one signed-in owner with one account: the
// session, the account list and the account's JMAP session object. The
// Playwright mocks and the preview's API both answer from here.

import { ACCOUNT_ADDRESS, UPSTREAM } from "./mail-corpus";
import { MAX_NAME_BYTES } from "./mail-folder-rules";
import { MAX_CONCURRENT_UPLOADS, MAX_UPLOAD_BYTES } from "./mail-sends";

export const SESSION_BODY = {
  user: { id: "user-1", login: "mira@example.com", name: "Mira", role: "owner" },
  organization: { id: "org-1", name: "mira@example.com" },
  signInProviders: [],
  passwordChangeRequired: false,
  privacyStrict: false,
};

export const SIGNED_OUT_BODY = { error: "unauthenticated" };

// One connected account, so the shell has something to show and keeps
// the session instead of sending it to the card.
export const ACCOUNT_LIST_BODY = {
  accounts: [
    {
      id: "acc-0",
      address: "mira@example.com",
      name: "example.com",
      provider: "generic",
      kind: "imap",
      authMethod: "password",
      stoppedCause: null,
      stoppedAt: null,
      createdAt: 1_778_750_400_000,
    },
  ],
  probeIntervalMinutes: 15,
};

const CORE_CAPABILITY = "urn:ietf:params:jmap:core";
const MAIL_CAPABILITY = "urn:ietf:params:jmap:mail";
// The vendor capability a bridge account advertises, which brings syncedEmails.
const HULIHO_CAPABILITY = "https://huliho.com/jmap";
const QUOTA_CAPABILITY = "urn:ietf:params:jmap:quota";
const SUBMISSION_CAPABILITY = "urn:ietf:params:jmap:submission";
// The account's submission limits as the bridge gives them: no delayed
// send and no extension (RFC 8621 section 1.3.2).
const SUBMISSION_LIMITS = { maxDelayedSend: 0, submissionExtensions: {} };
// How many objects one Email/set takes.
export const MAX_OBJECTS_IN_SET = 500;

export interface SessionShape {
  vendor: boolean;
  quota: boolean;
  maxObjectsInSet: number;
  topLevel: boolean;
}

// A native account without quotas that takes the default write and lets
// a folder be created at the top level.
export const PLAIN_SHAPE: SessionShape = {
  vendor: false,
  quota: false,
  maxObjectsInSet: MAX_OBJECTS_IN_SET,
  topLevel: true,
};

// The session object of an account on the proxy, as the worker reads
// it, with the account's mail limits the folder dialogs and the composer
// read (RFC 8621 section 1.3.1) and the submission capability; a bridge
// account carries the vendor capability as well, an account with
// quotas the quota capability.
export function jmapSessionBody(accountId: string, shape: SessionShape): object {
  const { vendor, quota, maxObjectsInSet, topLevel } = shape;
  const extra = {
    ...(vendor ? { [HULIHO_CAPABILITY]: {} } : {}),
    ...(quota ? { [QUOTA_CAPABILITY]: {} } : {}),
  };
  const limits = {
    maxSizeMailboxName: MAX_NAME_BYTES,
    mayCreateTopLevelMailbox: topLevel,
    maxSizeAttachmentsPerEmail: MAX_UPLOAD_BYTES,
  };
  return {
    capabilities: {
      [CORE_CAPABILITY]: {
        maxSizeUpload: MAX_UPLOAD_BYTES,
        maxConcurrentUpload: MAX_CONCURRENT_UPLOADS,
        maxSizeRequest: 1_048_576,
        maxConcurrentRequests: 4,
        maxCallsInRequest: 16,
        maxObjectsInGet: 500,
        maxObjectsInSet,
        collationAlgorithms: ["i;unicode-casemap"],
      },
      [MAIL_CAPABILITY]: {},
      [SUBMISSION_CAPABILITY]: {},
      ...extra,
    },
    accounts: {
      [UPSTREAM]: {
        name: ACCOUNT_ADDRESS,
        isPersonal: true,
        isReadOnly: false,
        accountCapabilities: {
          [MAIL_CAPABILITY]: limits,
          [SUBMISSION_CAPABILITY]: SUBMISSION_LIMITS,
          ...extra,
        },
      },
    },
    primaryAccounts: { [MAIL_CAPABILITY]: UPSTREAM, [SUBMISSION_CAPABILITY]: UPSTREAM },
    username: ACCOUNT_ADDRESS,
    apiUrl: `/api/jmap/${accountId}`,
    downloadUrl: `/api/jmap/${accountId}/download/{accountId}/{blobId}/{name}?type={type}`,
    uploadUrl: `/api/jmap/${accountId}/upload/{accountId}`,
    eventSourceUrl: `/api/jmap/${accountId}/events?types={types}&closeafter={closeafter}&ping={ping}`,
    state: "s1",
  };
}
