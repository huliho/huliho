// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CredentialError, rateLimited } from "./credentials";
import type { CredentialFailureCode } from "./credentials";
import { CSRF_HEADERS } from "./http";
import { z } from "./schema";

const PASSWORD_ENDPOINT = "/api/password";

// The window the server enforces; the form checks it first, so a typo
// never costs a round trip through the argon2 gate.
export const PASSWORD_MIN_CHARS = 12;
export const PASSWORD_MAX_CHARS = 128;

const errorBodySchema = z.object({ error: z.string() });

export interface PasswordChangeInput {
  // Left out in the forced step, where the one-time password already vouched.
  current?: string;
  new: string;
}

// Counted in code points, as the server counts.
export function fitsPasswordWindow(password: string): boolean {
  const length = Array.from(password).length;
  return length >= PASSWORD_MIN_CHARS && length <= PASSWORD_MAX_CHARS;
}

export async function changePassword(input: PasswordChangeInput): Promise<void> {
  let response: Response;
  try {
    response = await fetch(PASSWORD_ENDPOINT, {
      method: "PUT",
      headers: { "content-type": "application/json", ...CSRF_HEADERS },
      body: JSON.stringify(input),
    });
  } catch {
    throw new CredentialError("unavailable");
  }
  if (response.ok) {
    return;
  }
  if (response.status === 401) {
    throw new CredentialError(await unauthorizedCode(response));
  }
  if (response.status === 429) {
    throw rateLimited(response);
  }
  throw new CredentialError("unavailable");
}

// A 401 is a wrong current password or a session that is gone; the body says which.
async function unauthorizedCode(response: Response): Promise<CredentialFailureCode> {
  const { error } = errorBodySchema.parse(await response.json());
  return error === "invalid_credentials" ? "invalid_credentials" : "unauthenticated";
}
