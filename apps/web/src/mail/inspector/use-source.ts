// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQuery } from "@tanstack/react-query";

import { useOnline } from "../../shell/use-online";
import { readSource } from "./source";
import type { Source } from "./source";

// Where the source of one message comes from: its query key and the
// address of the whole message on the download route.
export interface SourceAsk {
  key: readonly unknown[];
  url: string;
}

export type SourceState =
  | { kind: "loading" }
  | { kind: "error"; retry: () => void }
  | { kind: "offline" }
  | { kind: "ready"; source: Source };

// The first part of a message's source, read once and kept for the
// session. A read that fails waits for Try again; one that fails with
// the device offline says so and is asked again when the network returns.
export function useSource(ask: SourceAsk): SourceState {
  const online = useOnline();
  const query = useQuery({
    queryKey: ask.key,
    queryFn: () => readSource(ask.url),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    networkMode: "always",
    refetchOnReconnect: true,
  });
  if (query.isError) {
    return online ? { kind: "error", retry: () => void query.refetch() } : { kind: "offline" };
  }
  if (query.data === undefined) {
    return { kind: "loading" };
  }
  return { kind: "ready", source: query.data };
}
