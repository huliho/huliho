// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  SenderPoliciesError,
  allowRemoteContent,
  blockRemoteContent,
  grantFor,
  grantLoads,
  senderKey,
} from "@huliho/core";
import type { Authentication, EmailHeader, RemoteContentGrant, SenderPolicy } from "@huliho/core";
import { queryKeys, senderPoliciesQueryOptions } from "@huliho/state";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";
import { useState } from "react";

import { useSessionEnded } from "../auth/use-session-ended";
import { toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

const REMOTE_CONTENT = "remoteContent";

// What the bar says of a message that names remote images.
export type RemoteBar = "blocked" | "once" | "always" | "failed";

export interface RemoteContent {
  // Whether the policies answered; the body waits for them, so a
  // message never shows blocked and loads a moment later. A list that
  // failed or waits for the network reads as none.
  ready: boolean;
  // Whether the sender's remote images load in this view.
  remote: boolean;
  bar: RemoteBar;
  // The sender the policy is keyed on, null for a message without one.
  sender: string | null;
  // Whether Always for this sender is offered: the message would load
  // under its own grant.
  canAlways: boolean;
  pending: boolean;
  loadOnce: () => void;
  allow: () => void;
  stop: () => void;
}

// A change to the sender's grant: the grant, or null to take it away.
interface GrantChange {
  sender: string;
  grant: RemoteContentGrant | null;
}

// The sender's grant among the policies; every policy row today carries
// the one key there is.
function grantOf(
  policies: readonly SenderPolicy[] | undefined,
  sender: string | null,
): RemoteContentGrant | undefined {
  if (sender === null) {
    return undefined;
  }
  return policies?.find((row) => row.sender === sender)?.value;
}

// The policies with the sender's grant changed.
function withGrant(policies: readonly SenderPolicy[], change: GrantChange): SenderPolicy[] {
  const others = policies.filter((row) => row.sender !== change.sender);
  return change.grant === null
    ? others
    : [...others, { sender: change.sender, key: REMOTE_CONTENT, value: change.grant }];
}

function barOf(granted: boolean, loads: boolean, once: boolean): RemoteBar {
  if (granted && loads) {
    return "always";
  }
  if (once) {
    return "once";
  }
  return granted ? "failed" : "blocked";
}

// A change to a sender's grant: into the cache first, to the server next,
// whatever the network does. A refusal puts the policies back as they
// stood and says so; a session that ended ends.
function useGrantWrite(locale: Locale) {
  const queryClient = useQueryClient();
  const sessionEnded = useSessionEnded(locale);
  return useMutation({
    networkMode: "always",
    mutationFn: (change: GrantChange) =>
      change.grant === null
        ? blockRemoteContent(change.sender)
        : allowRemoteContent(change.sender, change.grant),
    onMutate: async (change) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.senderPolicies });
      const before = queryClient.getQueryData<SenderPolicy[]>(queryKeys.senderPolicies) ?? [];
      queryClient.setQueryData(queryKeys.senderPolicies, withGrant(before, change));
      return before;
    },
    onError: (error, _change, before) => {
      if (error instanceof SenderPoliciesError && error.code === "unauthenticated") {
        sessionEnded();
        return;
      }
      queryClient.setQueryData(queryKeys.senderPolicies, before ?? []);
      void queryClient.invalidateQueries({ queryKey: queryKeys.senderPolicies });
      toastManager.add({ description: m.remote_save_failed({}, { locale }) });
    },
  });
}

type PoliciesRead = Pick<UseQueryResult<SenderPolicy[]>, "isFetched" | "isPaused">;

// Whether the policies answered, once and for good: with a list, with
// a failure or by waiting for the network. A message that shows keeps
// showing while the list is read again.
function useAnswered(policies: PoliciesRead): boolean {
  const [waited, setWaited] = useState(policies.isPaused);
  if (policies.isPaused && !waited) {
    setWaited(true);
  }
  return policies.isFetched || waited;
}

// Whether a message's remote images load and what the bar says: the
// sender's standing grant under its pin, or the one-message choice.
export function useRemoteContent(
  locale: Locale,
  email: EmailHeader,
  authentication: Authentication | null,
): RemoteContent {
  const policies = useQuery(senderPoliciesQueryOptions);
  const write = useGrantWrite(locale);
  const [once, setOnce] = useState(false);
  const ready = useAnswered(policies);
  const sender = senderKey(email.from);
  const grant = grantOf(policies.data, sender);
  const loads =
    grant !== undefined &&
    sender !== null &&
    authentication !== null &&
    grantLoads(grant, sender, authentication);
  const offered =
    sender !== null && authentication !== null ? grantFor(sender, authentication) : null;
  return {
    ready,
    remote: loads || once,
    bar: barOf(grant !== undefined, loads, once),
    sender,
    canAlways: grant === undefined && offered !== null,
    pending: write.isPending,
    loadOnce: () => {
      setOnce(true);
    },
    allow: () => {
      if (sender !== null && offered !== null) {
        write.mutate({ sender, grant: offered });
      }
    },
    stop: () => {
      if (sender !== null) {
        write.mutate({ sender, grant: null });
      }
    },
  };
}
