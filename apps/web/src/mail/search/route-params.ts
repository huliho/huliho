// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The search route's id, as the router registers it.
export const SEARCH_ROUTE_ID = "/signed-in/mail/$accountId/search";

// The search as the URL carries it, each param the string it holds; the
// search reads them through core's checks. `box` names the mailbox the
// search was opened from.
export interface SearchRoute {
  q?: string;
  in?: string;
  from?: string;
  after?: string;
  before?: string;
  unread?: string;
  starred?: string;
  attachment?: string;
  box?: string;
}

const NAMES = [
  "q",
  "in",
  "from",
  "after",
  "before",
  "unread",
  "starred",
  "attachment",
  "box",
] as const;

// The route's params: the known names that hold a string, nothing else.
export function searchRoute(raw: Record<string, unknown>): SearchRoute {
  const fields = new Map<string, unknown>(Object.entries(raw));
  return Object.fromEntries(
    NAMES.flatMap((name): [string, string][] => {
      const value = fields.get(name);
      return typeof value === "string" ? [[name, value]] : [];
    }),
  );
}

// Every param stays the string the URL writes, so `q=123` is text.
export function parseSearch(text: string): Record<string, string> {
  return Object.fromEntries(new URLSearchParams(text));
}

export function stringifySearch(search: Record<string, unknown>): string {
  const pairs = Object.entries(search).filter(
    (pair): pair is [string, string] => typeof pair[1] === "string",
  );
  const text = new URLSearchParams(pairs).toString();
  return text === "" ? "" : `?${text}`;
}
