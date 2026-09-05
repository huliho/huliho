// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CSRF_HEADERS } from "./http";
import { roleSchema } from "./role";
import type { Role } from "./role";
import { z } from "./schema";

const USERS_ENDPOINT = "/api/users";

// The server's bounds: a longer name is a paragraph, a longer sign-in
// name is no address.
export const USER_NAME_MAX_CHARS = 100;
export const LOGIN_MAX_BYTES = 254;

const userRowSchema = z.object({
  id: z.string(),
  name: z.string(),
  login: z.string(),
  role: roleSchema,
  lastActiveAt: z.number().nullable(),
});

const issuedPasswordSchema = z.object({
  oneTimePassword: z.string(),
  expiresAt: z.number(),
});

const createdUserSchema = issuedPasswordSchema.extend({ user: userRowSchema });

const errorBodySchema = z.object({ error: z.string() });

export type UserRow = z.infer<typeof userRowSchema>;
export type IssuedPassword = z.infer<typeof issuedPasswordSchema>;
export type CreatedUser = z.infer<typeof createdUserSchema>;

export interface NewUser {
  name: string;
  login: string;
  role: Role;
}

export type UsersFailureCode =
  "invalid_request" | "login_taken" | "forbidden" | "not_found" | "unauthenticated" | "unavailable";

const NAMED_FAILURES: readonly UsersFailureCode[] = [
  "invalid_request",
  "login_taken",
  "forbidden",
  "not_found",
  "unauthenticated",
];

export class UsersError extends Error {
  readonly code: UsersFailureCode;

  constructor(code: UsersFailureCode) {
    super(`users request failed: ${code}`);
    this.name = "UsersError";
    this.code = code;
  }
}

// Bounded in bytes and without whitespace, as the server checks; the
// Unicode property is the set the server's check reads.
export function fitsLogin(login: string): boolean {
  return (
    login.length > 0 &&
    !/\p{White_Space}/u.test(login) &&
    new TextEncoder().encode(login).length <= LOGIN_MAX_BYTES
  );
}

export async function fetchUsers(): Promise<UserRow[]> {
  const response = await fetch(USERS_ENDPOINT);
  if (!response.ok) {
    throw new Error(`the users request failed with status ${String(response.status)}`);
  }
  return z.array(userRowSchema).parse(await response.json());
}

export async function createUser(user: NewUser): Promise<CreatedUser> {
  const response = await post(USERS_ENDPOINT, user);
  return createdUserSchema.parse(await response.json());
}

export async function resetPassword(id: string): Promise<IssuedPassword> {
  const response = await post(`${USERS_ENDPOINT}/${encodeURIComponent(id)}/password-reset`);
  return issuedPasswordSchema.parse(await response.json());
}

async function post(url: string, body?: NewUser): Promise<Response> {
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", ...CSRF_HEADERS },
      body: body === undefined ? null : JSON.stringify(body),
    });
  } catch {
    throw new UsersError("unavailable");
  }
  if (response.ok) {
    return response;
  }
  throw new UsersError(await failureOf(response));
}

// The body names the refusal; anything unnamed reads as unavailable.
async function failureOf(response: Response): Promise<UsersFailureCode> {
  const body: unknown = await response.json().catch(() => null);
  const parsed = errorBodySchema.safeParse(body);
  const code = parsed.success ? parsed.data.error : "";
  return NAMED_FAILURES.find((named) => named === code) ?? "unavailable";
}
