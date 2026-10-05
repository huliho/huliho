// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The corpus the server's sanitizer runs, read for the browser tests:
// each payload as the sender wrote it, with the headers a case names.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { CorpusCase } from "./mail-corpus";

const CORPUS_DIR = join(import.meta.dirname, "../../../crates/server/tests/xss");
const MAIL_FILE = join(CORPUS_DIR, "mail.json");
const FIXTURES_FILE = join(CORPUS_DIR, "dompurify-expect.json");
// A case without a title is named by the start of its payload.
const TITLE_CHARS = 80;

interface Entry {
  title?: unknown;
  payload: string;
  headers?: unknown;
}

function isEntry(value: unknown): value is Entry {
  return (
    typeof value === "object" && value !== null && typeof Reflect.get(value, "payload") === "string"
  );
}

function isHeaders(value: unknown): value is Record<string, string[]> {
  return (
    typeof value === "object" &&
    value !== null &&
    Object.values(value).every(
      (lines) => Array.isArray(lines) && lines.every((line) => typeof line === "string"),
    )
  );
}

function caseOf(entry: Entry): CorpusCase {
  const made: CorpusCase = {
    title: typeof entry.title === "string" ? entry.title : entry.payload.slice(0, TITLE_CHARS),
    payload: entry.payload,
  };
  if (isHeaders(entry.headers)) {
    made.headers = entry.headers;
  }
  return made;
}

function casesOf(source: string): CorpusCase[] {
  const read: unknown = JSON.parse(source);
  if (!Array.isArray(read)) {
    throw new TypeError("the corpus holds no list");
  }
  return read.filter(isEntry).map(caseOf);
}

// The mail cases, then the DOMPurify fixtures.
export function corpusCases(): CorpusCase[] {
  return [
    ...casesOf(readFileSync(MAIL_FILE, "utf8")),
    ...casesOf(readFileSync(FIXTURES_FILE, "utf8")),
  ];
}
