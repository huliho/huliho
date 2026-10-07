// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Authentication, AuthenticationResults } from "@huliho/core";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import type { Locale } from "../../paraglide/runtime.js";
import { AuthenticationLine } from "./authentication-line";

afterEach(cleanup);

function parsed(results: Partial<AuthenticationResults>): Authentication {
  return {
    status: "parsed",
    results: {
      server: "mx.example.org",
      spf: "pass",
      dkim: "pass",
      dmarc: "fail",
      dmarcFrom: null,
      ...results,
    },
  };
}

function line(authentication: Authentication, locale: Locale = "en"): HTMLElement | null {
  const { container } = render(
    <AuthenticationLine locale={locale} authentication={authentication} />,
  );
  return container.querySelector("p");
}

test("the line is one sentence with the server in mono and a mark beside each word, the marks unheard", () => {
  const shown = line(parsed({}));
  expect(shown?.textContent).toBe(
    "Checked by mx.example.org: SPF passed, DKIM passed, DMARC failed.",
  );
  const host = shown?.querySelector("bdi");
  expect(host?.textContent).toBe("mx.example.org");
  const results = [...(shown?.querySelectorAll("[data-verdict]") ?? [])];
  expect(results.map((result) => result.getAttribute("data-verdict"))).toEqual([
    "pass",
    "pass",
    "fail",
  ]);
  for (const result of results) {
    expect(result.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  }
});

test("every result has its word: not checked for none and unknown for the rest", () => {
  const shown = line(parsed({ spf: "none", dkim: "unknown", dmarc: "pass" }));
  expect(shown?.textContent).toBe(
    "Checked by mx.example.org: SPF not checked, DKIM unknown, DMARC passed.",
  );
});

test("a header that names no server reads as the reader's own mail server, an empty name included", () => {
  expect(line(parsed({ server: null }))?.textContent).toBe(
    "Checked by your mail server: SPF passed, DKIM passed, DMARC failed.",
  );
  cleanup();
  expect(line(parsed({ server: "" }))?.textContent).toBe(
    "Checked by your mail server: SPF passed, DKIM passed, DMARC failed.",
  );
  cleanup();
  expect(screen.queryByText(/Checked by/)).toBeNull();
});

test("a message without the header or with one that cannot be read shows nothing", () => {
  expect(line({ status: "absent" })).toBeNull();
  cleanup();
  expect(line({ status: "unparseable" })).toBeNull();
});

test("the Dutch sentence keeps the same shape", () => {
  expect(line(parsed({ spf: "none", dkim: "unknown" }), "nl")?.textContent).toBe(
    "Gecontroleerd door mx.example.org: SPF niet gecontroleerd, DKIM onbekend, DMARC mislukt.",
  );
});
