// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CredentialError, rateLimited } from "./credentials";
import { CSRF_HEADERS } from "./http";
import { roleSchema } from "./role";
import { z } from "./schema";

const SESSION_ENDPOINT = "/api/session";

export const sessionInfoSchema = z.object({
  user: z.object({
    id: z.string(),
    login: z.string(),
    name: z.string(),
    role: roleSchema,
  }),
  organization: z.object({
    id: z.string(),
    name: z.string(),
  }),
  // True for a session opened with a one-time password, until the change lands.
  passwordChangeRequired: z.boolean(),
});

export type SessionInfo = z.infer<typeof sessionInfoSchema>;

export async function fetchSession(): Promise<SessionInfo | null> {
  const response = await fetch(SESSION_ENDPOINT);
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`the session request failed with status ${String(response.status)}`);
  }
  return sessionInfoSchema.parse(await response.json());
}

export async function signIn(login: string, password: string): Promise<void> {
  let response: Response;
  try {
    response = await fetch(SESSION_ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/json", ...CSRF_HEADERS },
      body: JSON.stringify({ login, password }),
    });
  } catch {
    throw new CredentialError("unavailable");
  }
  if (response.ok) {
    return;
  }
  if (response.status === 401) {
    throw new CredentialError("invalid_credentials");
  }
  if (response.status === 429) {
    throw rateLimited(response);
  }
  throw new CredentialError("unavailable");
}

export async function signOut(): Promise<void> {
  const response = await fetch(SESSION_ENDPOINT, {
    method: "DELETE",
    headers: CSRF_HEADERS,
  });
  if (!response.ok) {
    throw new Error(`the sign-out request failed with status ${String(response.status)}`);
  }
}
