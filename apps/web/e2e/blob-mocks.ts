// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BrowserContext } from "@playwright/test";

const DOWNLOAD_ROUTE = "**/api/jmap/*/download/**";
const REMOTE_ROUTE = "**/api/remote-image?*";
const PNG_TYPE = "image/png";
// The type of every blob the route does not serve as a picture.
const OCTET_STREAM = "application/octet-stream";

// A photo of 480 by 360, two bands of color, for every image the routes serve.
const PHOTO = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAeAAAAFoCAIAAAAAVb93AAAD5klEQVR42u3UQQ0AIAwAsfkXgA6eE4AGBKBkNrbQpArucZH3AdBQSABg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGLQKAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNIBBqwBg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBA3ww6LMXAA0ZNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0BIAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQAAatAoBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAEMUn6QFLdA47G8AAAAASUVORK5CYII=",
  "base64",
);

// A part declared an image whose bytes the server finds to be something
// else: its answer is a download, as the route makes of such bytes.
export const NOT_AN_IMAGE_NAME = "scan.png";

// The whole message the download route serves for a message blob.
const MESSAGE = "From: sender@example.test\r\nSubject: the message\r\n\r\nThe whole message.\r\n";
const MESSAGE_EXTENSION = ".eml";

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

// What the image proxy was asked: each remote image by the address it
// was given.
export interface Blobs {
  remote: string[];
}

// Answers the download route with the photo for an image asked as one
// and with a download for anything else, the message for a message
// name and plain bytes otherwise; the image proxy answers the photo and
// counts each address it was asked.
export async function mockBlobs(context: BrowserContext): Promise<Blobs> {
  const blobs: Blobs = { remote: [] };
  await context.route(DOWNLOAD_ROUTE, (route) => {
    const url = new URL(route.request().url());
    const type = url.searchParams.get("type") ?? "";
    const name = decodeURIComponent(url.pathname.split("/").pop() ?? "");
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
    return route.fulfill({
      body: name.endsWith(MESSAGE_EXTENSION) ? MESSAGE : BYTES,
      headers: {
        ...BLOB_HEADERS,
        "content-type": OCTET_STREAM,
        "content-disposition": `attachment; filename="${name}"`,
      },
    });
  });
  await context.route(REMOTE_ROUTE, (route) => {
    blobs.remote.push(new URL(route.request().url()).searchParams.get("url") ?? "");
    return route.fulfill({ body: PHOTO, headers: { ...BLOB_HEADERS, "content-type": PNG_TYPE } });
  });
  return blobs;
}
