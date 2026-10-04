// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Several times the longest header a server writes; a hostile one stops here.
export const AUTH_RESULTS_MAX_BYTES = 8192;
// A server reports a handful of methods; a header with more clauses is refused.
export const AUTH_RESULTS_MAX_CLAUSES = 32;
// An authserv-id is a host name by common practice (RFC 8601 section 2.5),
// which holds 255 octets at most (RFC 1035 section 2.3.4).
const SERVER_MAX_BYTES = 255;
// The version of the header and of every method read here (RFC 8601 section 2.6).
const VERSION = "1";
// Stands for the end of a comment or a quoted string that never closes.
const UNCLOSED = -1;

const FROM = "header.from";
// A line break ahead of a space or tab folds a line (RFC 5322 section 2.2.3).
const FOLD = /\r?\n(?=[ \t])/g;
const SPACE = /[ \t\r\n]/;
const KEYWORD_CHAR = /[\w-]/;
const BARE_CHAR = /[^ \t\r\n"]/;
const DIGITS = /^\d+$/;
const CONTROL = /\p{Cc}/u;
const QUOTED_PAIR = /\\(.)/gsu;
const NO_RESULT = /^[ \t\r\n]*none[ \t\r\n]*$/i;
const CLOSING_DOT = /\.$/;

// How one method came out, in the four words the summary shows.
export type AuthVerdict = "pass" | "fail" | "none" | "unknown";

export interface AuthenticationResults {
  // The authserv-id as written; null for a header that names none.
  server: string | null;
  spf: AuthVerdict;
  dkim: AuthVerdict;
  dmarc: AuthVerdict;
  // The header.from property of the dmarc clause, lowercased; null without one.
  dmarcFrom: string | null;
}

// A piece of a clause plus the index where the next piece starts.
interface Read<Value> {
  value: Value;
  end: number;
}

interface Spec {
  method: string;
  verdict: AuthVerdict;
}

interface Clause extends Spec {
  from: string | null;
}

interface Head {
  server: string | null;
  clauses: string[];
}

const VERDICTS = new Map<string, AuthVerdict>([
  ["pass", "pass"],
  ["fail", "fail"],
  ["softfail", "fail"],
  ["hardfail", "fail"],
  ["none", "none"],
]);
// Among several clauses of one method the first of these that occurs wins.
const BEST_FIRST: readonly AuthVerdict[] = ["pass", "fail", "unknown", "none"];

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

function runEnd(text: string, start: number, chars: RegExp): number {
  let at = start;
  while (chars.test(text.charAt(at))) {
    at += 1;
  }
  return at;
}

function skipSpace(text: string, start: number): number {
  return runEnd(text, start, SPACE);
}

// The index past the comment that opens at start. A comment nests and holds
// quoted pairs (RFC 5322 section 3.2.2); a counter keeps the walk flat.
function commentEnd(text: string, start: number): number {
  let depth = 0;
  let at = start;
  while (at < text.length) {
    const char = text.charAt(at);
    if (char === "(") {
      depth += 1;
    } else if (char === ")") {
      depth -= 1;
    } else if (char === "\\") {
      at += 1;
    }
    at += 1;
    if (depth === 0) {
      return at;
    }
  }
  return UNCLOSED;
}

// The index past the quoted string that opens at start (RFC 5322 section 3.2.4).
function quotedEnd(text: string, start: number): number {
  let at = start + 1;
  while (at < text.length) {
    const char = text.charAt(at);
    at += 1;
    if (char === '"') {
      return at;
    }
    if (char === "\\") {
      at += 1;
    }
  }
  return UNCLOSED;
}

// The index past what starts here: a whole comment, a whole quoted string or
// one character. A closing parenthesis that closes nothing cannot be read.
function pieceEnd(text: string, at: number): number {
  switch (text.charAt(at)) {
    case "(":
      return commentEnd(text, at);
    case '"':
      return quotedEnd(text, at);
    case ")":
      return UNCLOSED;
    default:
      return at + 1;
  }
}

// The header cut at every ";" outside quoted strings and comments, with a
// space in place of each comment and without an empty closing piece.
function splitClauses(text: string): string[] | null {
  const clauses: string[] = [];
  let clause = "";
  let at = 0;
  while (at < text.length) {
    const char = text.charAt(at);
    const end = pieceEnd(text, at);
    if (end === UNCLOSED) {
      return null;
    }
    if (char === ";") {
      clauses.push(clause);
      clause = "";
    } else {
      clause += char === "(" ? " " : text.slice(at, end);
    }
    at = end;
  }
  if (clauses.length === 0 || skipSpace(clause, 0) < clause.length) {
    clauses.push(clause);
  }
  return clauses;
}

function readQuoted(text: string, start: number): Read<string> | null {
  const end = quotedEnd(text, start);
  if (end === UNCLOSED) {
    return null;
  }
  return { value: text.slice(start + 1, end - 1).replaceAll(QUOTED_PAIR, "$1"), end };
}

// A keyword with the whitespace around it (RFC 8601 section 2.2).
function readWord(text: string, start: number): Read<string> | null {
  const from = skipSpace(text, start);
  const to = runEnd(text, from, KEYWORD_CHAR);
  return to === from ? null : { value: text.slice(from, to), end: skipSpace(text, to) };
}

// What follows an authserv-id: nothing, or whitespace and the version.
function fitsVersion(rest: string): boolean {
  if (skipSpace(rest, 0) === rest.length) {
    return true;
  }
  const version = readWord(rest, 0);
  return SPACE.test(rest.charAt(0)) && version?.value === VERSION && version.end === rest.length;
}

// The authserv-id of a first piece: one quoted string or one run of other
// characters, bounded and free of control characters.
function readServer(text: string): string | null {
  const start = skipSpace(text, 0);
  const bare = runEnd(text, start, BARE_CHAR);
  const id =
    text.charAt(start) === '"'
      ? readQuoted(text, start)
      : { value: text.slice(start, bare), end: bare };
  if (id === null || id.end === start || !fitsVersion(text.slice(id.end))) {
    return null;
  }
  return byteLength(id.value) > SERVER_MAX_BYTES || CONTROL.test(id.value) ? null : id.value;
}

// The server and its result clauses. The header Outlook.com and Exchange
// Online write names no server: its first piece is a clause already.
function readHead(pieces: string[]): Head | null {
  const [first = "", ...rest] = pieces;
  if (first.charAt(skipSpace(first, 0)) !== '"' && first.includes("=")) {
    return { server: null, clauses: pieces };
  }
  const server = readServer(first);
  return server === null ? null : { server, clauses: rest };
}

// Whether a method has the version read here; one that names none has it
// (RFC 8601 section 2.2).
function readVersion(text: string, at: number): Read<boolean> | null {
  if (text.charAt(at) !== "/") {
    return { value: true, end: at };
  }
  const version = readWord(text, at + 1);
  if (version === null || !DIGITS.test(version.value)) {
    return null;
  }
  return { value: version.value === VERSION, end: version.end };
}

function verdictOf(result: string): AuthVerdict {
  return VERDICTS.get(result.toLowerCase()) ?? "unknown";
}

// The method and its result. A result of another method version stays
// unread and counts as unknown (RFC 8601 section 2.6).
function readSpec(text: string): Read<Spec> | null {
  const method = readWord(text, 0);
  const version = method === null ? null : readVersion(text, method.end);
  if (method === null || version === null || text.charAt(version.end) !== "=") {
    return null;
  }
  const result = readWord(text, version.end + 1);
  if (result === null) {
    return null;
  }
  const verdict = version.value ? verdictOf(result.value) : "unknown";
  return { value: { method: method.value.toLowerCase(), verdict }, end: result.end };
}

// A property name is a ptype with a dot and a property. A bare word reads
// too: the reason and the action a server adds.
function readName(text: string, start: number): Read<string> | null {
  const ptype = readWord(text, start);
  if (ptype === null || text.charAt(ptype.end) !== ".") {
    return ptype;
  }
  const property = readWord(text, ptype.end + 1);
  if (property === null) {
    return null;
  }
  return { value: `${ptype.value}.${property.value}`, end: property.end };
}

// A property value: quoted strings and any other characters up to the next
// whitespace, wider than the grammar because servers write "/", "+" and "@".
function readValue(text: string, start: number): Read<string> | null {
  let value = "";
  let at = start;
  while (at < text.length && !SPACE.test(text.charAt(at))) {
    const char = text.charAt(at);
    const piece = char === '"' ? readQuoted(text, at) : { value: char, end: at + 1 };
    if (piece === null) {
      return null;
    }
    value += piece.value;
    at = piece.end;
  }
  return at === start ? null : { value, end: at };
}

// The properties that fill the rest of a clause, the first value per name.
function readProperties(text: string, start: number): Map<string, string> | null {
  const properties = new Map<string, string>();
  let at = start;
  while (at < text.length) {
    const name = readName(text, at);
    if (name === null || text.charAt(name.end) !== "=") {
      return null;
    }
    const value = readValue(text, skipSpace(text, name.end + 1));
    if (value === null) {
      return null;
    }
    const key = name.value.toLowerCase();
    if (!properties.has(key)) {
      properties.set(key, value.value);
    }
    at = skipSpace(text, value.end);
  }
  return properties;
}

function readClause(text: string): Clause | null {
  const spec = readSpec(text);
  const properties = spec === null ? null : readProperties(text, spec.end);
  if (spec === null || properties === null) {
    return null;
  }
  const from = properties.get(FROM)?.toLowerCase().replace(CLOSING_DOT, "");
  return { ...spec.value, from: from ?? null };
}

// Every clause but the bare "none" of a header without results; null when
// one breaks the grammar, when none is left to read or when a bound is passed.
function readClauses(texts: string[]): Clause[] | null {
  if (texts.length === 0 || texts.length > AUTH_RESULTS_MAX_CLAUSES) {
    return null;
  }
  const clauses: Clause[] = [];
  for (const text of texts.filter((clause) => !NO_RESULT.test(clause))) {
    const clause = readClause(text);
    if (clause === null) {
      return null;
    }
    clauses.push(clause);
  }
  return clauses;
}

function best(clauses: Clause[], method: string): AuthVerdict {
  const verdicts = new Set(
    clauses.filter((clause) => clause.method === method).map((clause) => clause.verdict),
  );
  return BEST_FIRST.find((verdict) => verdicts.has(verdict)) ?? "none";
}

// One DMARC check covers a message. A second dmarc clause is what a value
// the sender chose adds to a header that copies it unquoted, so a header
// with two is not read.
function summarize(server: string | null, clauses: Clause[]): AuthenticationResults | null {
  const [dmarc, second] = clauses.filter((clause) => clause.method === "dmarc");
  if (second !== undefined) {
    return null;
  }
  return {
    server,
    spf: best(clauses, "spf"),
    dkim: best(clauses, "dkim"),
    dmarc: dmarc?.verdict ?? "none",
    dmarcFrom: dmarc?.from ?? null,
  };
}

// The header as the receiving server wrote it; null for one that cannot be
// read or passes a bound.
export function parseAuthenticationResults(header: string): AuthenticationResults | null {
  if (header.length > AUTH_RESULTS_MAX_BYTES || byteLength(header) > AUTH_RESULTS_MAX_BYTES) {
    return null;
  }
  const pieces = splitClauses(header.replaceAll(FOLD, ""));
  const head = pieces === null ? null : readHead(pieces);
  const clauses = head === null ? null : readClauses(head.clauses);
  return head === null || clauses === null ? null : summarize(head.server, clauses);
}

// What the topmost header of a message says: none came, one came that
// cannot be read or one came with its results.
export type Authentication =
  | { status: "absent" }
  | { status: "unparseable" }
  | { status: "parsed"; results: AuthenticationResults };

// Every Authentication-Results header of a message, the topmost first.
// Only that one counts: a receiving server prepends its own, so a header
// the sender wrote sits below it.
export function authenticationOf(headers: readonly string[]): Authentication {
  const topmost = headers.at(0);
  if (topmost === undefined) {
    return { status: "absent" };
  }
  const results = parseAuthenticationResults(topmost);
  return results === null ? { status: "unparseable" } : { status: "parsed", results };
}
