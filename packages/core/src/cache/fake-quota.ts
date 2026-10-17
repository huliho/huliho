// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "../schema";
import type { FakeJmap } from "./fake-jmap";
import { MethodError, UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";
import type { Quota } from "./quota";

// One GiB in octets: the hard limit a built quota carries, so a used
// share reads as a plain fraction of it.
export const GIB = 1_073_741_824;

const getArgsSchema = z.object({
  accountId: z.string(),
  ids: z.nullish(z.array(z.string())),
});

// The quota methods answer only where the account lists the capability
// (RFC 8620 section 3.6.2).
export function quotaAccount(server: FakeJmap, accountId: unknown): void {
  if (!server.quotaAccount) {
    throw new MethodError("accountNotSupportedByMethod");
  }
  if (accountId !== UPSTREAM) {
    throw new MethodError("accountNotFound");
  }
}

// Quota/get (RFC 9425 section 4.2).
export function quotaGet(server: FakeJmap, raw: Args): Args {
  const args = getArgsSchema.safeParse(raw);
  if (!args.success) {
    throw new MethodError("invalidArguments");
  }
  quotaAccount(server, args.data.accountId);
  const asked = args.data.ids ?? [...server.quotas.keys()];
  const list = asked.flatMap((id): Quota[] => {
    const row = server.quotas.get(id);
    return row === undefined ? [] : [row];
  });
  return {
    accountId: UPSTREAM,
    state: String(server.sequence),
    list,
    notFound: asked.filter((id) => !server.quotas.has(id)),
  };
}

export function quota(id: string, facts: Partial<Quota> = {}): Quota {
  return {
    id,
    resourceType: "octets",
    used: 0,
    hardLimit: GIB,
    scope: "account",
    name: "User quota",
    types: ["Email"],
    warnLimit: null,
    softLimit: null,
    description: null,
    ...facts,
  };
}
