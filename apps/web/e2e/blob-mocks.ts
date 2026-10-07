// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BrowserContext, Route } from "@playwright/test";

const DOWNLOAD_ROUTE = "**/api/jmap/*/download/**";
const REMOTE_ROUTE = "**/api/remote-image?*";
const PNG_TYPE = "image/png";
// The type of every blob the route does not serve as a picture.
const OCTET_STREAM = "application/octet-stream";
const PARTIAL_CONTENT = 206;
const BAD_GATEWAY = 502;
// A range from byte zero, the one the route honors.
const RANGE = /^bytes=0-(\d+)$/;

// A photo of 480 by 360, two bands of color, for every image the routes serve.
const PHOTO = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAeAAAAFoCAIAAAAAVb93AAAD5klEQVR42u3UQQ0AIAwAsfkXgA6eE4AGBKBkNrbQpArucZH3AdBQSABg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGLQKAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNIBBqwBg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBA3ww6LMXAA0ZNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0BIAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQAAatAoBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAEMUn6QFLdA47G8AAAAASUVORK5CYII=",
  "base64",
);

// A part declared an image whose bytes the server finds to be something
// else: its answer is a download, as the route makes of such bytes.
export const NOT_AN_IMAGE_NAME = "scan.png";

// The whole message the download route serves for a message blob.
export const MESSAGE =
  "From: sender@example.test\r\nSubject: the message\r\n\r\nThe whole message.\r\n";
const MESSAGE_EXTENSION = ".eml";
// A long message runs this far past what the source view reads.
const LONG_MESSAGE_FACTOR = 3;

// The bytes of any other download: binary with no signature, so a
// browser keeps the name it was given instead of renaming it after a
// type it sniffed in the content.
const BYTES_LENGTH = 64;
const BYTES = Buffer.alloc(BYTES_LENGTH);

// The headers every blob answer carries, as the server's route sends them.
const BLOB_HEADERS = {
  "x-content-type-options": "nosniff",
  "content-security-policy": "default-src 'none'; sandbox",
  "cache-control": "private, no-store",
};

// What the blob routes were asked: each remote image by the address it
// was given and each range asked of a message; which messages are long,
// so a range from byte zero answers the first part of them; and which
// the route refuses, as a proxy whose upstream is down does.
export interface Blobs {
  remote: string[];
  ranges: string[];
  longMessages: Set<string>;
  refusedMessages: Set<string>;
}

// The whole message as the route serves it: the message, or the first
// part of a long one with the range it covers, as the route answers a
// blob whose length it knows.
function messageAnswer(route: Route, blobId: string, blobs: Blobs): Promise<void> {
  const headers = { ...BLOB_HEADERS, "content-type": OCTET_STREAM };
  if (blobs.refusedMessages.has(blobId)) {
    return route.fulfill({ status: BAD_GATEWAY, headers });
  }
  const range = route.request().headers()["range"];
  if (range !== undefined) {
    blobs.ranges.push(range);
  }
  const end = range === undefined ? null : RANGE.exec(range)?.[1];
  if (end === null || end === undefined || !blobs.longMessages.has(blobId)) {
    return route.fulfill({ body: MESSAGE, headers });
  }
  const first = Number(end) + 1;
  const whole = first * LONG_MESSAGE_FACTOR;
  return route.fulfill({
    status: PARTIAL_CONTENT,
    body: MESSAGE.padEnd(first, "x").slice(0, first),
    headers: { ...headers, "content-range": `bytes 0-${end}/${String(whole)}` },
  });
}

// Answers the download route with the photo for an image asked as one
// and with a download for anything else, the message for a message
// name and plain bytes otherwise; the image proxy answers the photo and
// counts each address it was asked.
export async function mockBlobs(context: BrowserContext): Promise<Blobs> {
  const blobs: Blobs = {
    remote: [],
    ranges: [],
    longMessages: new Set(),
    refusedMessages: new Set(),
  };
  await context.route(DOWNLOAD_ROUTE, (route) => {
    const url = new URL(route.request().url());
    const type = url.searchParams.get("type") ?? "";
    const [name = "", blobId = ""] = url.pathname.split("/").toReversed();
    if (type.startsWith("image/") && name !== NOT_AN_IMAGE_NAME) {
      return route.fulfill({
        body: PHOTO,
        headers: {
          ...BLOB_HEADERS,
          "content-type": PNG_TYPE,
          "content-disposition": `inline; filename="${name}"`,
        },
      });
    }
    if (decodeURIComponent(name).endsWith(MESSAGE_EXTENSION)) {
      return messageAnswer(route, blobId, blobs);
    }
    return route.fulfill({
      body: BYTES,
      headers: {
        ...BLOB_HEADERS,
        "content-type": OCTET_STREAM,
        "content-disposition": `attachment; filename="${decodeURIComponent(name)}"`,
      },
    });
  });
  await context.route(REMOTE_ROUTE, (route) => {
    blobs.remote.push(new URL(route.request().url()).searchParams.get("url") ?? "");
    return route.fulfill({ body: PHOTO, headers: { ...BLOB_HEADERS, "content-type": PNG_TYPE } });
  });
  return blobs;
}
