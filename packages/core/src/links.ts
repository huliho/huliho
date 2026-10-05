// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Why a link asks the reader before it opens: it points at the app
// itself, its text names another host than it opens or its host is an
// internationalized name, which can pass for another one.
export type LinkRisk =
  | { kind: "own"; path: string }
  | { kind: "mismatch"; text: string; host: string }
  | { kind: "international"; host: string };

// The longest link text read for a host name.
const TEXT_CHARS_MAX = 2048;

// The prefix of a label in its ASCII form (RFC 5890 section 2.3.2.1).
const ACE_PREFIX = "xn--";

// The label browsers and senders treat as optional in front of a host.
const WWW = "www.";

const WEB_SCHEME = /^https?:\/\//i;

// The format characters, which a reader never sees: joiners, direction
// marks and their kind (Unicode general category Cf).
const UNSEEN = /\p{Cf}/gu;

// A top-level label as a reader types it: letters alone, or an
// internationalized one in its ASCII form.
const TOP_LABEL = /^(?:[a-z]{2,}|xn--[a-z0-9-]+)$/;

// The host without the root's closing dot and without a leading www,
// so two spellings of one host compare equal.
function plain(host: string): string {
  const rooted = host.endsWith(".") ? host.slice(0, -1) : host;
  return rooted.startsWith(WWW) ? rooted.slice(WWW.length) : rooted;
}

function isBareHost(host: string): boolean {
  const labels = plain(host).split(".");
  return labels.length > 1 && TOP_LABEL.test(labels.at(-1) ?? "");
}

function parsed(value: string): URL | null {
  try {
    return new URL(value);
  } catch {
    return null;
  }
}

// A link's text as a reader sees it: without the characters that show
// nothing, which could split a host name or turn a sentence around.
export function linkText(text: string): string {
  return text.replace(UNSEEN, "").trim();
}

// The host a link's text names to a reader: a web address its host, or
// what stands in front of the at sign when it carries user information;
// a bare host name itself. Words and a mail address name none.
function hostNamed(shown: string): string | null {
  if (shown === "" || shown.length > TEXT_CHARS_MAX || /\s/.test(shown)) {
    return null;
  }
  if (WEB_SCHEME.test(shown)) {
    const url = parsed(shown);
    return url === null ? null : url.username || url.hostname;
  }
  const url = shown.includes("@") ? null : parsed(`https://${shown}`);
  return url !== null && isBareHost(url.hostname) ? url.hostname : null;
}

// The first reason a web link asks before it opens, null for a plain
// one. Hosts compare whole, in their ASCII lowercase form, apart from a
// leading www; `ownHost` is the host name the app runs on.
export function linkRisk(target: URL, text: string, ownHost: string): LinkRisk | null {
  if (target.hostname === ownHost) {
    return { kind: "own", path: target.pathname };
  }
  const shown = linkText(text);
  const named = hostNamed(shown);
  if (named !== null && plain(named) !== plain(target.hostname)) {
    return { kind: "mismatch", text: shown, host: target.hostname };
  }
  if (target.hostname.split(".").some((label) => label.startsWith(ACE_PREFIX))) {
    return { kind: "international", host: target.hostname };
  }
  return null;
}
