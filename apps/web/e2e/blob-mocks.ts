// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BrowserContext } from "@playwright/test";

const DOWNLOAD_ROUTE = "**/api/jmap/*/download/**";
const REMOTE_ROUTE = "**/api/remote-image?*";
const PNG_TYPE = "image/png";
const MESSAGE_TYPE = "message/rfc822";

// A red pixel, for every image the routes serve.
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==",
  "base64",
);

// The whole message the download route serves for a message blob.
const MESSAGE = "From: sender@example.test\r\nSubject: the message\r\n\r\nThe whole message.\r\n";

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

// Answers the download route with a pixel for an image and the message
// for anything else, and the image proxy with a pixel, each ask counted.
export async function mockBlobs(context: BrowserContext): Promise<Blobs> {
  const blobs: Blobs = { remote: [] };
  await context.route(DOWNLOAD_ROUTE, (route) => {
    const url = new URL(route.request().url());
    const type = url.searchParams.get("type") ?? "";
    if (type.startsWith("image/")) {
      return route.fulfill({ body: PNG, headers: { ...BLOB_HEADERS, "content-type": PNG_TYPE } });
    }
    const name = url.pathname.split("/").pop() ?? "message.eml";
    return route.fulfill({
      body: MESSAGE,
      headers: {
        ...BLOB_HEADERS,
        "content-type": MESSAGE_TYPE,
        "content-disposition": `attachment; filename="${name}"`,
      },
    });
  });
  await context.route(REMOTE_ROUTE, (route) => {
    blobs.remote.push(new URL(route.request().url()).searchParams.get("url") ?? "");
    return route.fulfill({ body: PNG, headers: { ...BLOB_HEADERS, "content-type": PNG_TYPE } });
  });
  return blobs;
}
