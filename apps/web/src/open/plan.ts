// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { classifyLinkUrl, linkRisk } from "@huliho/core";
import type { LinkRisk } from "@huliho/core";

import { readFragment } from "./fragment";
import type { OpenRequest } from "./fragment";
import { hasLinkKey } from "./link-key";

// Why the page asks before a link opens: a risk the link itself
// carries, or a link this device did not make.
export type Reason = LinkRisk | { kind: "unverified"; host: string };

// What the page does with the link in its fragment: nothing for one it
// cannot read, leave for the target at once, hand an address to the
// mail program or ask first.
export type Plan =
  | { kind: "invalid" }
  | { kind: "leave"; url: string; host: string }
  | { kind: "mail"; url: string }
  | { kind: "ask"; url: string; reason: Reason };

const INVALID: Plan = { kind: "invalid" };

// The plan for a page on another host. A link without this device's key
// always asks, so an address made elsewhere never forwards a visitor
// unseen. One that lost its text asks as well: the text is what shows
// that a link names another host.
function webPlan(address: string, request: OpenRequest, ownHost: string): Plan {
  const url = new URL(address);
  const risk = linkRisk(url, request.text ?? "", ownHost);
  if (risk !== null) {
    return { kind: "ask", url: address, reason: risk };
  }
  return hasLinkKey(request.key) && request.text !== null
    ? { kind: "leave", url: address, host: url.hostname }
    : { kind: "ask", url: address, reason: { kind: "unverified", host: url.hostname } };
}

// The plan for a fragment. A mail address without this device's key
// opens nothing.
export function planLink(hash: string, ownHost: string): Plan {
  const request = readFragment(hash);
  const target = request === null ? null : classifyLinkUrl(request.target, null);
  if (request === null || target === null || target.kind === "dropped") {
    return INVALID;
  }
  if (target.kind === "mail") {
    return hasLinkKey(request.key) ? { kind: "mail", url: target.url } : INVALID;
  }
  return webPlan(target.url, request, ownHost);
}

// The target as the page shows it, in three pieces with the one the
// reason is about in the middle: the path for a link at the app itself,
// the host for every other.
export function targetPieces(url: string, reason: Reason): [string, string, string] {
  const target = new URL(url);
  const tail = `${target.search}${target.hash}`;
  return reason.kind === "own"
    ? [target.origin, target.pathname, tail]
    : [`${target.protocol}//`, target.host, `${target.pathname}${tail}`];
}
