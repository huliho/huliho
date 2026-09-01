// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "zod";

// State-changing calls carry this header; the server refuses them without it.
const CSRF_HEADER = "x-requested-with";
const CSRF_VALUE = "huliho";

const SESSION_ENDPOINT = "/api/session";

// Shown when a rate-limited answer carries no usable Retry-After header.
const FALLBACK_RETRY_SECONDS = 60;

export const sessionInfoSchema = z.object({
  user: z.object({
    id: z.string(),
    login: z.string(),
    role: z.enum(["owner", "admin", "member"]),
  }),
  organization: z.object({
    id: z.string(),
    name: z.string(),
  }),
});

export type SessionInfo = z.infer<typeof sessionInfoSchema>;

export type SignInFailureCode = "invalid_credentials" | "rate_limited" | "unavailable";

export class SignInError extends Error {
  readonly code: SignInFailureCode;
  readonly retryAfterSeconds: number;

  constructor(code: SignInFailureCode, retryAfterSeconds = 0) {
    super(`sign-in failed: ${code}`);
    this.name = "SignInError";
    this.code = code;
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

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
      headers: {
        "content-type": "application/json",
        [CSRF_HEADER]: CSRF_VALUE,
      },
      body: JSON.stringify({ login, password }),
    });
  } catch {
    throw new SignInError("unavailable");
  }
  if (response.ok) {
    return;
  }
  if (response.status === 401) {
    throw new SignInError("invalid_credentials");
  }
  if (response.status === 429) {
    throw new SignInError("rate_limited", retryDelaySeconds(response));
  }
  throw new SignInError("unavailable");
}

export async function signOut(): Promise<void> {
  const response = await fetch(SESSION_ENDPOINT, {
    method: "DELETE",
    headers: { [CSRF_HEADER]: CSRF_VALUE },
  });
  if (!response.ok) {
    throw new Error(`the sign-out request failed with status ${String(response.status)}`);
  }
}

function retryDelaySeconds(response: Response): number {
  const seconds = Number(response.headers.get("retry-after") ?? "");
  return Number.isFinite(seconds) && seconds > 0 ? seconds : FALLBACK_RETRY_SECONDS;
}
