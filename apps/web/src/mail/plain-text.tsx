// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { classifyLinkUrl, quotedLines, unflow } from "@huliho/core";
import type { QuotedLine } from "@huliho/core";
import { LinkifyIt } from "linkify-it";
import type { ReactNode } from "react";

import { openHref } from "../open/fragment";
import styles from "./plain-text.module.css";

// Deeper quotes stand at the deepest level the design has.
const QUOTE_DEPTH_MAX = 3;

// A text is the sender's, so it grows neither the page nor the search
// without bound. Lines count as sent, before flowed lines join; fifty
// thousand of prose build inside the phone profile's task budget.
export const PLAIN_LINES_MAX = 50_000;
// Past the blocks the rest stands as one block with its quote marks as written.
export const PLAIN_RUNS_MAX = 2000;
// Past the links and past the searched characters an address stays text.
export const PLAIN_LINKS_MAX = 2000;
export const PLAIN_SEARCH_MAX = 256 * 1024;

const QUOTE_MARK = ">";
const LINE_BREAK = "\n";

// The text up to its last whole line inside the bound and whether more lines follow.
export function firstLines(text: string): { text: string; more: boolean } {
  let end = -1;
  for (let line = 0; line < PLAIN_LINES_MAX; line += 1) {
    end = text.indexOf(LINE_BREAK, end + 1);
    if (end === -1) {
      return { text, more: false };
    }
  }
  return { text: text.slice(0, end + 1), more: end + 1 < text.length };
}

// Finds the web and mail addresses in a text, a bare host name with a
// known top-level name among them; the one policy decides which of
// them become links.
const linkify = new LinkifyIt({ fuzzyLink: true });

// What a link of the message needs: the host the app runs on and the
// key of this device, which the open route asks of a link.
export interface LinkPolicy {
  ownHost: string;
  linkKey: string;
}

interface PlainTextProps {
  // At most the bound of lines; `firstLines` cuts a longer text.
  text: string;
  // The flowed parameters of the text, null for fixed text.
  flowed: { delSp: boolean } | null;
  links: LinkPolicy;
}

// Consecutive lines of one depth as one block, so a paragraph wraps as
// one; `start` is the number of its first line.
interface Run {
  start: number;
  depth: number;
  lines: string[];
}

// A line with the quote marks its depth stands for.
function asWritten(line: QuotedLine): string {
  return line.depth === 0 ? line.text : `${QUOTE_MARK.repeat(line.depth)} ${line.text}`;
}

function runsOf(lines: readonly QuotedLine[]): Run[] {
  const runs: Run[] = [];
  for (const [start, line] of lines.entries()) {
    const last = runs.at(-1);
    if (last !== undefined && runs.length === PLAIN_RUNS_MAX) {
      last.lines.push(asWritten(line));
    } else if (last !== undefined && last.depth === line.depth) {
      last.lines.push(line.text);
    } else if (runs.length === PLAIN_RUNS_MAX - 1) {
      runs.push({ start, depth: 0, lines: [asWritten(line)] });
    } else {
      runs.push({ start, depth: line.depth, lines: [line.text] });
    }
  }
  return runs;
}

// The links a message may still draw and the characters it may still search.
interface Budget {
  links: number;
  search: number;
}

// The start of a text that is searched for addresses: all of it while
// the budget lasts, else the whole lines that still fit.
function searched(text: string, budget: Budget): string {
  if (budget.links === 0) {
    return "";
  }
  const fits = text.slice(0, budget.search);
  const head = fits.length === text.length ? fits : fits.slice(0, fits.lastIndexOf("\n") + 1);
  budget.search -= head.length;
  return head;
}

// Where a found address opens, with the target the title shows; null
// for one the policy drops.
function addressOf(url: string, text: string, links: LinkPolicy): [string, string] | null {
  const target = classifyLinkUrl(url, links.ownHost);
  if (target.kind === "dropped") {
    return null;
  }
  const href = openHref({ target: target.url, text, key: links.linkKey });
  return href === null ? null : [href, target.url];
}

// The text with its addresses as links while the budget lasts; what
// the policy drops stays text.
function linked(text: string, links: LinkPolicy, budget: Budget): ReactNode[] {
  const pieces: ReactNode[] = [];
  let at = 0;
  for (const match of linkify.match(searched(text, budget)) ?? []) {
    const address = budget.links > 0 ? addressOf(match.url, match.raw, links) : null;
    if (address === null) {
      continue;
    }
    budget.links -= 1;
    pieces.push(text.slice(at, match.index));
    pieces.push(
      <a
        key={match.index}
        href={address[0]}
        target="_blank"
        rel="noopener noreferrer"
        title={address[1]}
      >
        {match.raw}
      </a>,
    );
    at = match.lastIndex;
  }
  pieces.push(text.slice(at));
  return pieces;
}

// The text of a run as its block draws it. A line break that ends a
// block draws no line, so a run that ends on an empty line gets one more.
function drawn(run: Run): string {
  const text = run.lines.join("\n");
  return run.lines.at(-1) === "" ? `${text}\n` : text;
}

// The level a run draws at; past the deepest the quotes stay at it.
function levelOf(run: Run): number {
  return Math.min(run.depth, QUOTE_DEPTH_MAX);
}

// The runs as the quotes they stand in: a run at its own level, deeper
// runs after it inside one quote of the next, so a reply and what it
// quotes nest as written and each level draws one bar.
function quoted(runs: readonly Run[], draw: (run: Run) => ReactNode): ReactNode[] {
  let at = 0;
  const level = (depth: number): ReactNode[] => {
    const nodes: ReactNode[] = [];
    for (let run = runs.at(at); run !== undefined && levelOf(run) >= depth; run = runs.at(at)) {
      if (levelOf(run) === depth) {
        nodes.push(draw(run));
        at += 1;
      } else {
        nodes.push(
          <blockquote key={`quote-${String(run.start)}`} className={styles.quote}>
            {level(depth + 1)}
          </blockquote>,
        );
      }
    }
    return nodes;
  };
  return level(0);
}

// A plain message in the card: its lines as written, the soft breaks of
// flowed text joined, quoted text as a quote per level and every
// address a link through the open route. Built as elements, never as
// markup; the text is mail content, so it reads in its own direction.
export function PlainText({ text, flowed, links }: PlainTextProps) {
  const lines = flowed === null ? quotedLines(text) : unflow(text, flowed.delSp);
  const budget: Budget = { links: PLAIN_LINKS_MAX, search: PLAIN_SEARCH_MAX };
  return (
    <div dir="auto" className={styles.text}>
      {quoted(runsOf(lines), (run) => (
        <div key={run.start} data-depth={run.depth === 0 ? undefined : levelOf(run)}>
          {linked(drawn(run), links, budget)}
        </div>
      ))}
    </div>
  );
}
