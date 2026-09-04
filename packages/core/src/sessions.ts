// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CSRF_HEADERS } from "./http";
import { z } from "./schema";

const SESSIONS_ENDPOINT = "/api/sessions";

export const deviceSchema = z.object({
  browser: z.string().nullable(),
  os: z.string().nullable(),
  phone: z.boolean(),
  installed: z.boolean(),
});

export const sessionRowSchema = z.object({
  id: z.string(),
  current: z.boolean(),
  device: deviceSchema,
  address: z.string().nullable(),
  createdAt: z.number(),
  lastSeenAt: z.number(),
});

export type Device = z.infer<typeof deviceSchema>;
export type SessionRow = z.infer<typeof sessionRowSchema>;

export interface RevokeOptions {
  // A revoke that fires while the page unloads needs keepalive to finish.
  keepalive?: boolean;
}

export async function fetchSessions(): Promise<SessionRow[]> {
  const response = await fetch(SESSIONS_ENDPOINT);
  if (!response.ok) {
    throw new Error(`the sessions request failed with status ${String(response.status)}`);
  }
  return z.array(sessionRowSchema).parse(await response.json());
}

export async function revokeSession(id: string, options: RevokeOptions = {}): Promise<void> {
  await revoke(`${SESSIONS_ENDPOINT}/${encodeURIComponent(id)}`, options);
}

export async function revokeOtherSessions(options: RevokeOptions = {}): Promise<void> {
  await revoke(SESSIONS_ENDPOINT, options);
}

// A row that is already gone is the outcome asked for, so 404 passes.
async function revoke(url: string, options: RevokeOptions): Promise<void> {
  const response = await fetch(url, {
    method: "DELETE",
    headers: CSRF_HEADERS,
    keepalive: options.keepalive === true,
  });
  if (!response.ok && response.status !== 404) {
    throw new Error(`the revoke request failed with status ${String(response.status)}`);
  }
}
