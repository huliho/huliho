// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { sameAddress } from "../address";
import { answer, callsFor } from "../jmap/calls";
import type { JmapClient } from "../jmap/client";
import { identitySchema } from "../jmap/schemas";
import type { Identity } from "../jmap/schemas";
import { z } from "../schema";

const IDENTITY_CALL = "i";

// An Identity/get answer as far as the client reads it: the list.
const identityAnswerSchema = z.object({ list: z.array(identitySchema) });

// The identity a draft goes out as: the one whose address is the
// account's own, else the first the server answers (RFC 8621 section 6);
// null for an account that sends with none or takes no send.
export async function readIdentity(client: JmapClient, address: string): Promise<Identity | null> {
  const session = await client.session();
  if (!session.submission) {
    return null;
  }
  const responses = await client.request([callsFor(session.accountId).identityGet(IDENTITY_CALL)]);
  const { list } = answer(responses, IDENTITY_CALL, identityAnswerSchema);
  return list.find((identity) => sameAddress(identity.email, address)) ?? list[0] ?? null;
}
