// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The Huliho account id the client is built on, and the upstream id
// the session object names for it.
export const ACCOUNT = "acc-1";
export const UPSTREAM = "u1";

export type Args = Record<string, unknown>;
export type Invocation = [string, Args, string];

// A method that answers an error in place of its response (RFC 8620
// section 3.6.2).
export class MethodError extends Error {
  readonly type: string;

  constructor(type: string) {
    super(type);
    this.type = type;
  }
}
