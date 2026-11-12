// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { filledTemplate } from "../download";
import { CSRF_HEADERS } from "../http";
import { z } from "../schema";
import type { JmapClient } from "./client";
import { uploadAnswerSchema } from "./schemas";
import type { UploadAnswer } from "./schemas";

// The type a file goes up as where it declares none (RFC 2046 section 4.5.1).
const OCTET_STREAM = "application/octet-stream";

// How often a busy lane is asked again before the upload gives up; the
// route itself waits for a lane before it answers 429.
const LANE_TRIES = 3;

// The wait before the next try where a 429 names none in its Retry-After.
const LANE_WAIT_DEFAULT_MS = 1000;
const MS_PER_SECOND = 1000;

const OK_FIRST = 200;
const OK_LAST = 299;
// The route answers 401 for a session that ended and for a credential
// the upstream refused; the word in the body tells the two apart.
const UNAUTHORIZED = 401;
const SESSION_ENDED = "unauthenticated";
const apiErrorSchema = z.object({ error: z.string() });
const REQUEST_TIMEOUT = 408;
const PAYLOAD_TOO_LARGE = 413;
const TOO_MANY_REQUESTS = 429;
// RFC 4918 section 11.5: the bridge's store found no room.
const INSUFFICIENT_STORAGE = 507;

// How an upload failed: the file past the account's limit, the store
// without room, the body gone silent on the way, the lanes still busy,
// the caller giving it up, the session ended or the route out of reach.
export type UploadFailureCode =
  "too_large" | "over_quota" | "timeout" | "busy" | "aborted" | "unauthenticated" | "unavailable";

export class UploadError extends Error {
  readonly code: UploadFailureCode;

  constructor(code: UploadFailureCode) {
    super(`upload failed: ${code}`);
    this.name = "UploadError";
    this.code = code;
  }
}

export interface UploadOptions {
  // The bytes sent so far and the bytes there are, as they leave.
  progress?: (sent: number, total: number) => void;
  signal?: AbortSignal;
}

interface Reply {
  status: number;
  body: string;
  retryAfter: string | null;
}

// The wait a 429 asks for, in milliseconds.
function waitOf(retryAfter: string | null): number {
  const seconds = Number(retryAfter ?? "");
  return Number.isFinite(seconds) && seconds > 0 ? seconds * MS_PER_SECOND : LANE_WAIT_DEFAULT_MS;
}

// The body as JSON, undefined where it is none.
function jsonOf(body: string): unknown {
  try {
    return JSON.parse(body);
  } catch {
    return undefined;
  }
}

function failureOf(reply: Reply): UploadFailureCode {
  switch (reply.status) {
    case PAYLOAD_TOO_LARGE:
      return "too_large";
    case INSUFFICIENT_STORAGE:
      return "over_quota";
    case REQUEST_TIMEOUT:
      return "timeout";
    case TOO_MANY_REQUESTS:
      return "busy";
    case UNAUTHORIZED: {
      const named = apiErrorSchema.safeParse(jsonOf(reply.body));
      return named.success && named.data.error === SESSION_ENDED
        ? "unauthenticated"
        : "unavailable";
    }
    default:
      return "unavailable";
  }
}

// The answer of a route that took the file, or the failure it named.
function answerOf(reply: Reply): UploadAnswer {
  if (reply.status < OK_FIRST || reply.status > OK_LAST) {
    throw new UploadError(failureOf(reply));
  }
  const body = jsonOf(reply.body);
  const read = uploadAnswerSchema.safeParse(body);
  if (!read.success) {
    throw new UploadError("unavailable");
  }
  return read.data;
}

// One POST of the file. XMLHttpRequest is the one browser API that
// reports the bytes of a request body as they leave.
function post(url: string, file: Blob, { progress, signal }: UploadOptions): Promise<Reply> {
  return new Promise((settle, refuse) => {
    if (signal?.aborted === true) {
      refuse(new UploadError("aborted"));
      return;
    }
    const request = new XMLHttpRequest();
    request.open("POST", url);
    request.setRequestHeader("content-type", file.type === "" ? OCTET_STREAM : file.type);
    for (const [name, value] of Object.entries(CSRF_HEADERS)) {
      request.setRequestHeader(name, value);
    }
    const abort = (): void => {
      request.abort();
    };
    // The caller's signal outlives the request, so the listener leaves with it.
    const done = (): void => {
      signal?.removeEventListener("abort", abort);
    };
    request.upload.addEventListener("progress", (event) => {
      progress?.(event.loaded, file.size);
    });
    request.addEventListener("load", () => {
      done();
      settle({
        status: request.status,
        body: request.responseText,
        retryAfter: request.getResponseHeader("retry-after"),
      });
    });
    request.addEventListener("error", () => {
      done();
      refuse(new UploadError("unavailable"));
    });
    request.addEventListener("abort", () => {
      done();
      refuse(new UploadError("aborted"));
    });
    signal?.addEventListener("abort", abort, { once: true });
    request.send(file);
  });
}

function pause(milliseconds: number, signal: AbortSignal | undefined): Promise<void> {
  return new Promise((settle, refuse) => {
    const abort = (): void => {
      clearTimeout(timer);
      refuse(new UploadError("aborted"));
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", abort);
      settle();
    }, milliseconds);
    signal?.addEventListener("abort", abort, { once: true });
  });
}

async function tried(
  url: string,
  file: Blob,
  options: UploadOptions,
  tries: number,
): Promise<UploadAnswer> {
  const reply = await post(url, file, options);
  if (reply.status !== TOO_MANY_REQUESTS || tries >= LANE_TRIES) {
    return answerOf(reply);
  }
  await pause(waitOf(reply.retryAfter), options.signal);
  return tried(url, file, options, tries + 1);
}

// Sends one file to the account's upload route (RFC 8620 section 6.1)
// and answers its blob id, type and size. A file past the account's
// `maxSizeUpload` is refused before a byte leaves; a 429 is waited out
// as the answer asks, a few times at most.
export async function uploadFile(
  client: JmapClient,
  file: Blob,
  options: UploadOptions = {},
): Promise<UploadAnswer> {
  const session = await client.session();
  if (file.size > session.uploadLimits.maxSizeUpload) {
    throw new UploadError("too_large");
  }
  const url = filledTemplate(session.uploadUrl, new Map([["accountId", session.accountId]]));
  return tried(url, file, options, 1);
}
