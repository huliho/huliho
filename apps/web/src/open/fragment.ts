// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { linkText } from "@huliho/core";

// The route every link of a mail opens through. The link itself rides
// the fragment, which a browser never sends to a server.
const OPEN_ROUTE = "/open";

// The longest fragment a link carries, in bytes.
export const FRAGMENT_BYTES_MAX = 4096;

// The link text a fragment carries: enough to read a host name from.
const TEXT_CHARS_MAX = 256;

// A link as its fragment carries it: where it leads, the text the mail
// showed for it and the key of the device that made it. The text is
// null for a link whose fragment had no room for it.
export interface OpenRequest {
  target: string;
  text: string | null;
  key: string;
}

function fragment(fields: Record<string, string>): string {
  return new URLSearchParams(fields).toString();
}

// The address a link in a mail opens, on this origin. The text is cut
// from what a reader sees of it. A fragment that would pass its bound
// carries no text, which the route reads as a link to ask about; a
// target that alone passes the bound has no address.
export function openHref({ target, text, key }: OpenRequest): string | null {
  const fields = { k: key, u: target };
  const shown = text === null ? [] : [linkText(text).slice(0, TEXT_CHARS_MAX)];
  const candidates = [...shown.map((t) => fragment({ ...fields, t })), fragment(fields)];
  const fits = candidates.find((candidate) => candidate.length <= FRAGMENT_BYTES_MAX);
  return fits === undefined ? null : `${OPEN_ROUTE}#${fits}`;
}

// The link a fragment carries; null for one past the bound or without
// a target.
export function readFragment(hash: string): OpenRequest | null {
  const body = hash.startsWith("#") ? hash.slice(1) : hash;
  if (body.length > FRAGMENT_BYTES_MAX) {
    return null;
  }
  const fields = new URLSearchParams(body);
  const target = fields.get("u");
  if (target === null || target === "") {
    return null;
  }
  return { target, text: fields.get("t"), key: fields.get("k") ?? "" };
}
