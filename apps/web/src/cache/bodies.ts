// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readBody } from "@huliho/core";
import type { BodyDetail, JmapClient, MailStore } from "@huliho/core";

// The body of one email with where its parts download from. It is read
// outside the account's lock, so it waits for no poll; its row and the
// eviction behind it are batches of their own.
export async function bodyDetail(
  client: JmapClient,
  store: MailStore,
  emailId: string,
  options: { large: boolean },
): Promise<BodyDetail | null> {
  const body = await readBody(client, store, emailId, options);
  if (body === null) {
    return null;
  }
  const session = await client.session();
  return { body, download: { template: session.downloadUrl, accountId: session.accountId } };
}
