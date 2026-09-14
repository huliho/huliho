// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Shown when a rate-limited answer carries no usable Retry-After header.
const FALLBACK_RETRY_SECONDS = 60;

// How a credential check fails. Sign-in and the password change share
// the limiter and the argon2 gate, so they fail the same ways.
export type CredentialFailureCode =
  "invalid_credentials" | "rate_limited" | "unauthenticated" | "unavailable";

export class CredentialError extends Error {
  readonly code: CredentialFailureCode;
  readonly retryAfterSeconds: number;

  constructor(code: CredentialFailureCode, retryAfter = 0) {
    super(`credential check failed: ${code}`);
    this.name = "CredentialError";
    this.code = code;
    this.retryAfterSeconds = retryAfter;
  }
}

// The seconds a rate-limited answer asks the caller to wait; the
// fallback when the header is unusable.
export function retryAfterSeconds(response: Response): number {
  const seconds = Number(response.headers.get("retry-after") ?? "");
  return Number.isFinite(seconds) && seconds > 0 ? seconds : FALLBACK_RETRY_SECONDS;
}

export function rateLimited(response: Response): CredentialError {
  return new CredentialError("rate_limited", retryAfterSeconds(response));
}
