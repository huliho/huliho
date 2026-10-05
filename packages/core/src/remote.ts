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

// What the target of a link in a mail is, by the same policy: a page on
// another host, an address to write to or nothing a link may carry.
export type LinkTarget =
  { kind: "web"; url: string } | { kind: "mail"; url: string } | { kind: "dropped" };

// A page on another host, without the user information a sender may
// write in front of the host to pass it off as another one.
function webTarget(url: URL): LinkTarget {
  const bare = new URL(url);
  bare.username = "";
  bare.password = "";
  return { kind: "web", url: bare.href };
}

// The class of a link's target. A relative URL, a URL on the instance's
// own host, an /api/ path on any host and every scheme but http, https
// and mailto are dropped.
export function classifyLinkUrl(value: string, ownHost: string | null): LinkTarget {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return { kind: "dropped" };
  }
  switch (url.protocol) {
    case "http:":
    case "https:":
      return onInstance(url, ownHost) ? { kind: "dropped" } : webTarget(url);
    case "mailto:":
      return { kind: "mail", url: url.href };
    default:
      return { kind: "dropped" };
  }
}

// Where the frame loads a remote image from once the reader allowed
// it: the server fetches it, so the sender never sees the reader.
export function remoteImageUrl(url: string): string {
  return `${REMOTE_IMAGE_ROUTE}?url=${encodeURIComponent(url)}`;
}
