// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

const REMOTE_IMAGE_ROUTE = "/api/remote-image";

// The path under which the server answers; a URL there would reach the
// API with the reader's cookies.
const API_PATH = "/api/";

// The media type prefix a data: URL needs on an image.
const IMAGE_TYPE = "image/";

// What the URL of an image in a mail is, by the one policy the server's
// sanitizer holds as well: a part of the message itself, an image on
// another host, an image carried in the URL or nothing the frame loads.
export type ImageSource =
  | { kind: "cid"; cid: string }
  | { kind: "remote"; url: string }
  | { kind: "data" }
  | { kind: "dropped" };

const DROPPED: ImageSource = { kind: "dropped" };

// A Content-ID as the part carries it; the URL form percent-encodes it
// (RFC 2392 section 2).
function contentId(url: URL): ImageSource {
  try {
    return { kind: "cid", cid: decodeURIComponent(url.pathname) };
  } catch {
    return DROPPED;
  }
}

function onInstance(url: URL, ownHost: string | null): boolean {
  return url.pathname.startsWith(API_PATH) || url.hostname === ownHost;
}

// The class of an image URL from a mail. A relative URL, a URL on the
// instance's own host, an /api/ path on any host and every scheme but
// cid, http, https and a data image are dropped; `ownHost` is the host
// name the app runs on.
export function classifyImageUrl(value: string, ownHost: string | null): ImageSource {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return DROPPED;
  }
  switch (url.protocol) {
    case "http:":
    case "https:":
      return onInstance(url, ownHost) ? DROPPED : { kind: "remote", url: url.href };
    case "cid:":
      return contentId(url);
    case "data:":
      return url.pathname.slice(0, IMAGE_TYPE.length).toLowerCase() === IMAGE_TYPE
        ? { kind: "data" }
        : DROPPED;
    default:
      return DROPPED;
  }
}

// Where the frame loads a remote image from once the reader allowed
// it: the server fetches it, so the sender never sees the reader.
export function remoteImageUrl(url: string): string {
  return `${REMOTE_IMAGE_ROUTE}?url=${encodeURIComponent(url)}`;
}
