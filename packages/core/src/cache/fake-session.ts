// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  CORE_CAPABILITY,
  HULIHO_CAPABILITY,
  MAIL_CAPABILITY,
  QUOTA_CAPABILITY,
  SUBMISSION_CAPABILITY,
} from "../jmap/schemas";
import type { FakeJmap } from "./fake-jmap";
import { ACCOUNT, UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";

// How many uploads go at once, as the bridge allows them.
const CONCURRENT_UPLOADS = 2;
// The account's submission limits as the bridge gives them: no delayed
// send and no extension (RFC 8621 section 1.3.2).
const SUBMISSION_LIMITS = { maxDelayedSend: 0, submissionExtensions: {} };

// The capabilities a request may name to this fake.
export function knownCapabilities(server: FakeJmap): Set<string> {
  return new Set([
    CORE_CAPABILITY,
    MAIL_CAPABILITY,
    HULIHO_CAPABILITY,
    ...(server.quota ? [QUOTA_CAPABILITY] : []),
    ...(server.submission ? [SUBMISSION_CAPABILITY] : []),
  ]);
}

// The session object as the proxy serves it for the fake's account.
export function sessionOf(server: FakeJmap): Args {
  const core = {
    maxSizeUpload: server.uploadLimit,
    maxConcurrentUpload: CONCURRENT_UPLOADS,
    maxSizeRequest: 1_048_576,
    maxConcurrentRequests: 4,
    maxCallsInRequest: server.maxCallsInRequest,
    maxObjectsInGet: server.maxObjectsInGet,
    maxObjectsInSet: server.maxObjectsInSet,
    collationAlgorithms: ["i;unicode-casemap"],
  };
  const vendor = server.vendor ? { [HULIHO_CAPABILITY]: {} } : {};
  const quota = server.quota ? { [QUOTA_CAPABILITY]: {} } : {};
  const accountQuota = server.quotaAccount ? quota : {};
  const submission = server.submission ? { [SUBMISSION_CAPABILITY]: {} } : {};
  const accountSubmission = server.submission ? { [SUBMISSION_CAPABILITY]: SUBMISSION_LIMITS } : {};
  const mail = {
    mayCreateTopLevelMailbox: server.topLevel,
    maxSizeMailboxName: server.maxSizeMailboxName,
    maxSizeAttachmentsPerEmail: server.uploadLimit,
  };
  return {
    capabilities: {
      [CORE_CAPABILITY]: core,
      [MAIL_CAPABILITY]: {},
      ...vendor,
      ...quota,
      ...submission,
    },
    accounts: {
      [UPSTREAM]: {
        name: "sanne@example.test",
        isPersonal: true,
        isReadOnly: server.readOnly,
        accountCapabilities: {
          [MAIL_CAPABILITY]: mail,
          ...vendor,
          ...accountQuota,
          ...accountSubmission,
        },
      },
    },
    primaryAccounts: { [MAIL_CAPABILITY]: UPSTREAM },
    username: "sanne@example.test",
    apiUrl: `/api/jmap/${ACCOUNT}`,
    downloadUrl: `/api/jmap/${ACCOUNT}/download/{accountId}/{blobId}/{name}?type={type}`,
    uploadUrl: `/api/jmap/${ACCOUNT}/upload/{accountId}`,
    eventSourceUrl: `/api/jmap/${ACCOUNT}/events?types={types}&closeafter={closeafter}&ping={ping}`,
    state: server.sessionState,
  };
}
