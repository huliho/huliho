// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The upload route of the fake (RFC 8620 section 6.1), reached through a
// stand-in for XMLHttpRequest that a test puts in the global's place: a
// file within the limit lands under a new blob id, a larger one answers
// 413 as the route does, a scripted answer goes out first.

import type { FakeJmap } from "./fake-jmap";
import { ACCOUNT, UPSTREAM } from "./fake-wire";

const UPLOAD_URL = `/api/jmap/${ACCOUNT}/upload/${UPSTREAM}`;
const OCTET_STREAM = "application/octet-stream";
const OK = 200;
const FORBIDDEN = 403;
const NOT_FOUND = 404;
const PAYLOAD_TOO_LARGE = 413;
// How much of a file the first progress event reports.
const PROGRESS_SHARE = 2;

// An answer a test scripts for the next upload: a status with the
// Retry-After it carries and the word its body names, or a request the
// network drops.
export type UploadReply =
  { status: number; retryAfter?: string; error?: string } | { status: "dropped" };

// One upload the route saw.
export interface UploadSeen {
  url: string;
  headers: Headers;
  size: number;
}

interface Progress {
  loaded: number;
  total: number;
  lengthComputable: boolean;
}

interface Answer {
  status: number;
  body: unknown;
  retryAfter: string | null;
}

function routed(server: FakeJmap, seen: UploadSeen): Answer {
  const scripted = server.uploadReplies.shift();
  if (scripted?.status === "dropped") {
    throw new Error("dropped");
  }
  if (seen.headers.get("x-requested-with") !== "huliho") {
    return { status: FORBIDDEN, body: { error: "missing_csrf_header" }, retryAfter: null };
  }
  if (seen.url !== UPLOAD_URL) {
    return { status: NOT_FOUND, body: { error: "not_found" }, retryAfter: null };
  }
  if (scripted !== undefined) {
    return {
      status: scripted.status,
      body: { error: scripted.error ?? "scripted" },
      retryAfter: scripted.retryAfter ?? null,
    };
  }
  if (seen.size > server.uploadLimit) {
    return { status: PAYLOAD_TOO_LARGE, body: { error: "too_large" }, retryAfter: null };
  }
  const blobId = `upload-${String(server.uploads.size + 1)}`;
  const type = seen.headers.get("content-type") ?? OCTET_STREAM;
  server.uploads.set(blobId, { type, size: seen.size });
  return {
    status: OK,
    body: { accountId: UPSTREAM, blobId, type, size: seen.size },
    retryAfter: null,
  };
}

// A progress event as XMLHttpRequest's upload sends it.
function progressOf(progress: Progress): Event {
  return Object.assign(new Event("progress"), progress);
}

// The stand-in for XMLHttpRequest with the members the upload client
// uses, answered by the server it was made for.
export class FakeUploadRequest extends EventTarget {
  readonly upload = new EventTarget();
  status = 0;
  responseText = "";
  private readonly server: FakeJmap;
  private url = "";
  private readonly headers = new Headers();
  private retryAfter: string | null = null;
  private settled = false;

  constructor(server: FakeJmap) {
    super();
    this.server = server;
  }

  open(_method: string, url: string): void {
    this.url = url;
  }

  setRequestHeader(name: string, value: string): void {
    this.headers.set(name, value);
  }

  getResponseHeader(name: string): string | null {
    return name.toLowerCase() === "retry-after" ? this.retryAfter : null;
  }

  send(body: Blob): void {
    const go = (): void => {
      this.arrive(body);
    };
    if (this.server.holdUploads) {
      this.server.heldUploads.push(go);
    } else {
      setTimeout(go, 0);
    }
  }

  abort(): void {
    if (!this.settled) {
      this.settled = true;
      this.dispatchEvent(new Event("abort"));
    }
  }

  private arrive(body: Blob): void {
    if (this.settled) {
      return;
    }
    const seen = { url: this.url, headers: new Headers(this.headers), size: body.size };
    this.server.uploaded.push(seen);
    const total = body.size;
    const half = Math.floor(total / PROGRESS_SHARE);
    this.upload.dispatchEvent(progressOf({ loaded: half, total, lengthComputable: true }));
    this.upload.dispatchEvent(progressOf({ loaded: total, total, lengthComputable: true }));
    this.settled = true;
    let answer: Answer;
    try {
      answer = routed(this.server, seen);
    } catch {
      this.dispatchEvent(new Event("error"));
      return;
    }
    this.status = answer.status;
    this.responseText = JSON.stringify(answer.body);
    this.retryAfter = answer.retryAfter;
    this.dispatchEvent(new Event("load"));
  }
}

// The constructor a test puts in the global's place of XMLHttpRequest,
// bound to one server.
export function uploadRequestOf(server: FakeJmap): new () => FakeUploadRequest {
  return class extends FakeUploadRequest {
    constructor() {
      super(server);
    }
  };
}
