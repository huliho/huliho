// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { scopeOf } from "@huliho/core";
import type { MailCache, Mailbox, SearchParams } from "@huliho/core";
import { searchWindowQueryOptions } from "@huliho/state";

import type { ListSource } from "../thread-list";

interface ResultsContext {
  cache: MailCache;
  accountId: string;
  locale: string;
  mailboxes: readonly Mailbox[];
  result: { actsFrom: string | null };
}

// The results of the search under `key`, page by page from the worker's
// snapshot: no first sync and no new mail, each row acting from its own
// mailbox and naming it where the scope spans more than one.
export function resultsSource(context: ResultsContext, key: string, ran: SearchParams): ListSource {
  const { cache, accountId, locale } = context;
  return {
    page: (page) => searchWindowQueryOptions(cache, accountId, key, { locale, page }),
    firstSync: null,
    reveal: () => undefined,
    openMailboxId: context.result.actsFrom,
    worded: scopeOf(ran, context.mailboxes).kind !== "one",
  };
}
