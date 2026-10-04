// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import {
  AUTH_RESULTS_MAX_BYTES,
  AUTH_RESULTS_MAX_CLAUSES,
  type AuthenticationResults,
  type AuthVerdict,
  parseAuthenticationResults,
} from "./auth-results";

// The longest authserv-id: the 255 octets of a host name (RFC 1035 section 2.3.4).
const SERVER_MAX_BYTES = 255;
// Runs per property, ten times the default: a parse is cheap and the input space wide.
const RUNS = 1000;
// Keeps generated lists, strings and nesting short, far under the byte bound.
const SMALL = 4;
// Method clauses per bare none in a generated header, so most headers carry results.
const METHOD_WEIGHT = 6;

const parse = parseAuthenticationResults;

const GMAIL = [
  " mx.google.com;",
  "       dkim=pass header.i=@example.com header.s=20260101 header.b=K/abc+d;",
  "       spf=pass (google.com: domain of bounce@example.com designates 203.0.113.7 as permitted sender) smtp.mailfrom=bounce@example.com;",
  "       dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=example.com",
].join("\r\n");

const MICROSOFT = [
  " spf=pass (sender IP is 203.0.113.7)",
  " smtp.mailfrom=shop.example; dkim=pass (signature was verified)",
  " header.d=shop.example;dmarc=pass action=none",
  " header.from=shop.example;compauth=pass reason=100",
].join("\r\n");

const RSPAMD = [
  " mail.example.org;",
  "\tdkim=pass header.d=shop.example header.s=sel1 header.b=AbCd1234;",
  "\tdmarc=pass (policy=none) header.from=shop.example;",
  "\tspf=pass (mail.example.org: domain of news@shop.example designates 203.0.113.9 as permitted sender) smtp.mailfrom=news@shop.example",
].join("\n");

const RESULTS: [string, AuthVerdict][] = [
  ["pass", "pass"],
  ["fail", "fail"],
  ["softfail", "fail"],
  ["hardfail", "fail"],
  ["none", "none"],
  ["neutral", "unknown"],
  ["policy", "unknown"],
  ["temperror", "unknown"],
  ["permerror", "unknown"],
];

const NO_RESULTS = { spf: "none", dkim: "none", dmarc: "none", dmarcFrom: null } as const;

function results(over: Partial<AuthenticationResults>): AuthenticationResults {
  return { server: "example.com", ...NO_RESULTS, ...over };
}

function passes(server: string | null, dmarcFrom: string): AuthenticationResults {
  return { server, spf: "pass", dkim: "pass", dmarc: "pass", dmarcFrom };
}

// A header of an exact length in filler characters, the filler inside a comment.
function sized(length: number, filler: string): string {
  const [open, close] = ["example.com; spf=pass (", ")"];
  return `${open}${filler.repeat(length - open.length - close.length)}${close}`;
}

function clauses(count: number): string {
  return Array.from({ length: count }, () => "dkim=pass").join("; ");
}

test("a Gmail header reads as its server and three passes", () => {
  expect(parse(GMAIL)).toEqual(passes("mx.google.com", "example.com"));
});

test("the header Outlook.com and Exchange Online write names no server and reads from its first clause on", () => {
  expect(parse(MICROSOFT)).toEqual(passes(null, "shop.example"));
});

test("an rspamd header reads the same with its lines folded by a bare line feed", () => {
  expect(parse(RSPAMD)).toEqual(passes("mail.example.org", "shop.example"));
});

test("a header that reports no result reads as none three times (RFC 8601 section 2.2)", () => {
  expect(parse("authserv; none")).toEqual(results({ server: "authserv" }));
  expect(parse(" example.com ; NONE ;")).toEqual(results({}));
});

test("a quoted authserv-id reads without its quotes and quoted pairs (RFC 8601 section 2.2)", () => {
  expect(parse('"mail example"; none')).toEqual(results({ server: "mail example" }));
  expect(parse('"a=b;c(d"; none')).toEqual(results({ server: "a=b;c(d" }));
  expect(parse(String.raw`"a\"b\\c"; none`)).toEqual(results({ server: String.raw`a"b\c` }));
  expect(parse('"mail\r\n example"; none')).toEqual(results({ server: "mail example" }));
});

test("an empty quoted authserv-id reads as the empty string, never as no server", () => {
  expect(parse('""; spf=pass')).toEqual(results({ server: "", spf: "pass" }));
});

test("version 1 after the authserv-id reads and any other version is refused (RFC 8601 section 2.6)", () => {
  expect(parse("example.com 1; spf=pass")).toEqual(results({ spf: "pass" }));
  expect(parse("example.com (v) 1 (v); spf=pass")).toEqual(results({ spf: "pass" }));
  expect(parse('"example.com" 1; spf=pass')).toEqual(results({ spf: "pass" }));
  expect(parse("example.com 2; spf=pass")).toBeNull();
  expect(parse("example.com 01; spf=pass")).toBeNull();
  expect(parse("example.com 1 1; spf=pass")).toBeNull();
  expect(parse('"example.com"1; spf=pass')).toBeNull();
});

test("a method of another version than 1 counts as unknown (RFC 8601 section 2.6)", () => {
  expect(parse("example.com; dkim/1=pass; spf / 1 = pass")).toEqual(
    results({ spf: "pass", dkim: "pass" }),
  );
  expect(parse("example.com; dkim/2=pass; spf/10=fail")).toEqual(
    results({ spf: "unknown", dkim: "unknown" }),
  );
  expect(parse("example.com; dkim/x=pass")).toBeNull();
});

test("a comment may nest and hides its separators (RFC 8601 section 2.2)", () => {
  const header = [
    String.raw`example.com (one (two (three)) \) still "one);`,
    "spf=pass (x; dmarc=fail header.from=other.example) smtp.mailfrom=shop.example;",
    "dkim (a) = (b) fail (c) header (d) . (e) d (f) = (g) shop.example;",
    "dmarc=pass(p=NONE)header.from=shop.example",
  ].join(" ");
  expect(parse(header)).toEqual(
    results({ spf: "pass", dkim: "fail", dmarc: "pass", dmarcFrom: "shop.example" }),
  );
});

test("a quoted value keeps its separator and reads without its quotes (RFC 8601 section 2.2)", () => {
  const header = [
    'example.com; spf=pass smtp.mailfrom="a; dmarc=fail header.from=other.example"@shop.example',
    'dmarc=pass header.from="Shop.Example"',
  ].join(";");
  expect(parse(header)).toEqual(results({ spf: "pass", dmarc: "pass", dmarcFrom: "shop.example" }));
});

test("whitespace and folds may stand around every separator (RFC 8601 section 2.2)", () => {
  const header =
    " example.com\r\n\t;\r\n spf = pass\r\n smtp . mailfrom = shop.example ;\r\n dmarc\t=\tfail ; \r\n";
  expect(parse(header)).toEqual(results({ spf: "pass", dmarc: "fail" }));
});

test("a property without a ptype and a value outside the strict grammar still read", () => {
  const header = [
    "example.com; dkim=pass header.i=@shop.example header.b=K/abc+d=",
    " spf=softfail smtp.mailfrom=news@shop.example reason=100",
    ' dmarc=fail action=none reason="p=reject" header.from=shop.example',
  ].join(";");
  expect(parse(header)).toEqual(
    results({ spf: "fail", dkim: "pass", dmarc: "fail", dmarcFrom: "shop.example" }),
  );
});

test("the best of several clauses of one method wins: pass over fail over unknown over none", () => {
  expect(parse("example.com; dkim=fail; dkim=pass; dkim=neutral")).toEqual(
    results({ dkim: "pass" }),
  );
  expect(parse("example.com; dkim=none; dkim=temperror; dkim=fail")).toEqual(
    results({ dkim: "fail" }),
  );
  expect(parse("example.com; spf=none; spf=permerror")).toEqual(results({ spf: "unknown" }));
  expect(parse("example.com; spf=none; spf=none; dkim=pass")).toEqual(results({ dkim: "pass" }));
});

test("a header with two dmarc clauses is not read, wherever a copied value put the second", () => {
  const injected =
    "spf=pass smtp.mailfrom=a; dmarc=pass header.from=bank.example@evil.example; dmarc=fail";
  expect(parse(`example.com; ${injected}`)).toBeNull();
  expect(parse("example.com; dmarc=pass; DMARC=pass header.from=other.example")).toBeNull();
  expect(parse("example.com; dmarc=pass header.from=a.example header.from=b.example")).toEqual(
    results({ dmarc: "pass", dmarcFrom: "a.example" }),
  );
});

test.each(RESULTS)("the result %s reads as %s", (result, verdict) => {
  expect(parse(`example.com; spf=${result}`)).toEqual(results({ spf: verdict }));
});

test("method, result and property names read in any case and the domain reads in lowercase without its closing dot", () => {
  expect(
    parse("example.com; SPF=Pass; DKIM=SoftFail; Dmarc=PASS Header.From=Shop.Example."),
  ).toEqual(results({ spf: "pass", dkim: "fail", dmarc: "pass", dmarcFrom: "shop.example" }));
});

test("a header of the largest size reads and one byte more is refused", () => {
  expect(parse(sized(AUTH_RESULTS_MAX_BYTES, "x"))).toEqual(results({ spf: "pass" }));
  expect(parse(sized(AUTH_RESULTS_MAX_BYTES + 1, "x"))).toBeNull();
});

test("the size bound counts bytes, so a header of fewer characters can pass it", () => {
  const header = sized(AUTH_RESULTS_MAX_BYTES, "é");
  expect(header).toHaveLength(AUTH_RESULTS_MAX_BYTES);
  expect(parse(header)).toBeNull();
});

test("the largest count of clauses reads and one clause more is refused", () => {
  const most = clauses(AUTH_RESULTS_MAX_CLAUSES);
  expect(parse(`example.com; ${most};`)).toEqual(results({ dkim: "pass" }));
  expect(parse(`example.com; ${most}; dkim=pass`)).toBeNull();
  expect(parse(most)).toEqual(results({ server: null, dkim: "pass" }));
  expect(parse(`${most}; dkim=pass`)).toBeNull();
});

test("an authserv-id of 255 bytes reads and one byte more is refused", () => {
  const longest = "a".repeat(SERVER_MAX_BYTES);
  expect(parse(`${longest}; none`)).toEqual(results({ server: longest }));
  expect(parse(`"${longest}"; none`)).toEqual(results({ server: longest }));
  expect(parse(`${longest}a; none`)).toBeNull();
  expect(parse(`"${longest}a"; none`)).toBeNull();
  expect(parse(`${"a".repeat(SERVER_MAX_BYTES - 1)}é; none`)).toBeNull();
});

test.each([
  "exam\u0000ple.com",
  "example.com\u007F",
  "exam\u0085ple.com",
  '"exam\tple.com"',
  '"exam\r\nple.com"',
])("the authserv-id %j holds a control character and is refused", (server) => {
  expect(parse(`${server}; none`)).toBeNull();
});

test.each([
  "",
  " \t\r\n ",
  ";",
  ";;",
  "example.com",
  "example.com;",
  "example.com; ; spf=pass",
  "example.com extra; none",
  'example.com "extra"; none',
  "example.com; spf=pass (unclosed",
  String.raw`example.com; spf=pass (escaped \)`,
  "example.com; spf=pass smtp.mailfrom=shop.example)",
  'example.com; spf=pass header.i="unclosed',
  "example.com; spf",
  "example.com; spf=",
  "example.com; =pass",
  "example.com; spf=pa$$",
  "example.com; spf/=pass",
  "example.com; spf=pass header.d",
  "example.com; spf=pass header.d=",
  "example.com; spf=pass header.=x",
  "example.com; spf=pass .d=x",
  "example.com; spf=pass header.a.b=x",
  "example.com; spf=pass.header.d=x",
  "example.com; none spf=pass",
])("%j is refused as unreadable", (header) => {
  expect(parse(header)).toBeNull();
});

interface ClauseModel {
  method: string;
  verdict: AuthVerdict;
  from: string | null;
}

// What a generator wrote next to what a reader finds in it.
interface Written<Model> {
  text: string;
  model: Model;
}

const METHODS = ["spf", "dkim", "dmarc", "arc", "compauth", "iprev", "dkim-atps", "x_ptr"];
const SERVERS = ["mx.example.com", "example-auth", "ms1.newyork.example.com", "mx.example/7"];
const NAMES = ["header.d", "header.i", "header.b", "smtp.mailfrom", "policy.dkim-rules", "action"];
const VALUES = ["shop.example", "@shop.example", "K/abc+d=", "news@shop.example", "<>", "100"];
// A header.from value as written next to the domain it reads as.
const DOMAINS: [string, string][] = [
  ["shop.example", "shop.example"],
  ["Shop.Example", "shop.example"],
  ["shop.example.", "shop.example"],
  ["OTHER.EXAMPLE.", "other.example"],
];
const SCRAPS = ["(", ")", '"', "\\", ";", "=", ".", "/", " ", "\r\n ", "none", "dmarc", "pass"];
const WEAKEST_FIRST: AuthVerdict[] = ["none", "unknown", "fail", "pass"];
const WORDS = new Set<string>(WEAKEST_FIRST);

function join(parts: string[]): string {
  return parts.join("");
}

function escape(text: string, specials: RegExp): string {
  return text.replaceAll(specials, (char) => `\\${char}`);
}

function quote(text: string): string {
  return `"${escape(text, /["\\]/g)}"`;
}

const blank = fc.constantFrom(" ", "\t", "  ", "\r\n ", "\n\t");
const commentText = fc.string({ maxLength: SMALL }).map((text) => escape(text, /[()\\]/g));
const { comment } = fc.letrec<{ comment: string }>((tie) => ({
  comment: fc
    .array(fc.oneof({ maxDepth: SMALL }, commentText, tie("comment")), { maxLength: SMALL })
    .map((parts) => `(${join(parts)})`),
}));
const gap = fc.array(fc.oneof(blank, comment), { maxLength: SMALL }).map(join);
const space = fc.tuple(fc.oneof(blank, comment), gap).map(join);

const value = fc.oneof(fc.constantFrom(...VALUES), fc.string({ maxLength: SMALL }).map(quote));
const plainProperty = fc
  .tuple(fc.constantFrom(...NAMES), value)
  .map(([name, text]) => ({ name, text, from: null }));
const fromProperty = fc
  .tuple(fc.mixedCase(fc.constant("header.from")), fc.constantFrom(...DOMAINS), fc.boolean())
  .map(([name, [written, from], quoted]) => ({
    name,
    text: quoted ? quote(written) : written,
    from,
  }));
const property = fc
  .tuple(space, fc.oneof(plainProperty, fromProperty), gap, gap, gap, gap)
  .map(([lead, { name, text, from }, a, b, c, d]) => ({
    text: `${lead}${name.split(".").join(`${a}.${b}`)}${c}=${d}${text}`,
    from,
  }));

const version = fc.oneof(
  fc.constant({ text: "", read: true }),
  fc.tuple(gap, gap, fc.constantFrom("1", "2")).map(([a, b, digits]) => ({
    text: `${a}/${b}${digits}`,
    read: digits === "1",
  })),
);
const result = fc
  .tuple(fc.constantFrom(...RESULTS), fc.boolean())
  .map(([[word, verdict], upper]) => ({ word: upper ? word.toUpperCase() : word, verdict }));

const methodClause: fc.Arbitrary<Written<ClauseModel | null>> = fc
  .tuple(
    fc.mixedCase(fc.constantFrom(...METHODS)),
    version,
    result,
    fc.array(property, { maxLength: SMALL }),
    fc.tuple(gap, gap, gap, gap),
  )
  .map(([method, { text, read }, { word, verdict }, properties, [a, b, c, d]]) => ({
    text: `${a}${method}${text}${b}=${c}${word}${join(properties.map((one) => one.text))}${d}`,
    model: {
      method: method.toLowerCase(),
      verdict: read ? verdict : "unknown",
      from: properties.find((one) => one.from !== null)?.from ?? null,
    },
  }));
const noResult: fc.Arbitrary<Written<ClauseModel | null>> = fc
  .tuple(gap, fc.mixedCase(fc.constant("none")), gap)
  .map(([a, word, b]) => ({ text: `${a}${word}${b}`, model: null }));

const id = fc.oneof(
  fc.constantFrom(...SERVERS).map((name) => ({ text: name, model: name })),
  fc.string({ maxLength: SMALL }).map((name) => ({ text: quote(name), model: name })),
);
const headerVersion = fc.oneof(fc.constant(""), fc.tuple(space, fc.constant("1")).map(join));
// Empty for the header that names no server; its first clause then opens the header.
const server: fc.Arbitrary<Written<string | null>> = fc.oneof(
  fc.constant({ text: "", model: null }),
  fc
    .tuple(gap, id, headerVersion, gap)
    .map(([a, { text, model }, one, b]) => ({ text: `${a}${text}${one}${b};`, model })),
);

const header = fc
  .tuple(
    server,
    fc.array(
      fc.oneof(
        { arbitrary: methodClause, weight: METHOD_WEIGHT },
        { arbitrary: noResult, weight: 1 },
      ),
      { minLength: 1, maxLength: SMALL },
    ),
    fc.constantFrom("", ";", "; \r\n"),
  )
  .filter(([{ model }, written]) => model !== null || written.at(0)?.model !== null)
  .map(([{ text, model }, written, tail]) => ({
    text: `${text}${written.map((one) => one.text).join(";")}${tail}`,
    server: model,
    models: written.map((one) => one.model).filter((one) => one !== null),
  }))
  .filter(({ text }) => text.length <= AUTH_RESULTS_MAX_BYTES);

// A header the grammar writes with a few arbitrary characters dropped into it.
const spliced = fc
  .tuple(header, fc.nat(), fc.string({ unit: "binary", maxLength: SMALL }))
  .map(([{ text }, at, piece]) => {
    const cut = at % (text.length + 1);
    return `${text.slice(0, cut)}${piece}${text.slice(cut)}`;
  });
const anything = fc.oneof(
  fc.string({ unit: "binary" }),
  fc.array(fc.constantFrom(...SCRAPS)).map(join),
  spliced,
);

function bestOf(models: ClauseModel[], method: string): AuthVerdict {
  const ranks = models
    .filter((model) => model.method === method)
    .map((model) => WEAKEST_FIRST.indexOf(model.verdict));
  return WEAKEST_FIRST.at(Math.max(0, ...ranks)) ?? "none";
}

function expected(name: string | null, models: ClauseModel[]): AuthenticationResults | null {
  const [dmarc, second] = models.filter((model) => model.method === "dmarc");
  if (second !== undefined) {
    return null;
  }
  return {
    server: name,
    spf: bestOf(models, "spf"),
    dkim: bestOf(models, "dkim"),
    dmarc: dmarc?.verdict ?? "none",
    dmarcFrom: dmarc?.from ?? null,
  };
}

function wellFormed(read: AuthenticationResults | null): boolean {
  if (read === null) {
    return true;
  }
  const serverBytes = new TextEncoder().encode(read.server ?? "").length;
  const verdicts = [read.spf, read.dkim, read.dmarc];
  return serverBytes <= SERVER_MAX_BYTES && verdicts.every((verdict) => WORDS.has(verdict));
}

test("any string at all answers null or a bounded result and never throws", () => {
  fc.assert(
    fc.property(anything, (text) => {
      expect(wellFormed(parse(text))).toBe(true);
    }),
    { numRuns: RUNS },
  );
});

test("every header the grammar writes reads as the results it was built from (RFC 8601 section 2.2)", () => {
  fc.assert(
    fc.property(header, ({ text, server: name, models }) => {
      expect(parse(text)).toEqual(expected(name, models));
    }),
    { numRuns: RUNS },
  );
});
