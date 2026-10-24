// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QuotaReading } from "@huliho/core";
import { expect, test } from "vitest";

import { quotaWords } from "./quota-words";

const KB = 1000;
const MB = 1000 ** 2;
const GB = 1000 ** 3;
// A quota a server sets in steps of 1024, as Gmail's 15 GiB.
const GIB = 1024 ** 3;

function octets(used: number, limit: number, warn = false): QuotaReading {
  return { unit: "octets", used, limit, share: Math.min(1, used / limit), warn };
}

function count(used: number, limit: number): QuotaReading {
  return { unit: "count", used, limit, share: Math.min(1, used / limit), warn: false };
}

test("a storage quota reads both figures in the limit's unit, one decimal below ten", () => {
  expect(quotaWords(octets(4.2 * GB, 10 * GB), "en")).toEqual({
    words: "4.2 of 10 GB",
    detail: "Mail storage: 4.2 GB of 10 GB used",
    warning: null,
  });
  expect(quotaWords(octets(12.4 * GB, 100 * GB), "en").words).toBe("12 of 100 GB");
  expect(quotaWords(octets(312 * MB, 15 * GB), "en").words).toBe("0.3 of 15 GB");
  expect(quotaWords(octets(300 * KB, 800 * KB), "en").words).toBe("300 of 800 kB");
});

test("the units step by 1000, so a limit set in steps of 1024 reads larger", () => {
  expect(quotaWords(octets(GIB, 15 * GIB), "en").words).toBe("1.1 of 16 GB");
});

test("a quota past its warn limit carries the warning word", () => {
  expect(quotaWords(octets(9.1 * GB, 10 * GB, true), "en")).toEqual({
    words: "9.1 of 10 GB",
    detail: "Mail storage: 9.1 GB of 10 GB used",
    warning: "Almost full",
  });
});

test("a server past its limit or at the largest figure still reads plainly", () => {
  expect(quotaWords(octets(10.5 * GB, 10 * GB, true), "en").words).toBe("11 of 10 GB");
  const largest = Number.MAX_SAFE_INTEGER;
  expect(quotaWords(octets(largest, largest), "en").words).toBe("9 of 9 PB");
});

test("a count quota reads in messages", () => {
  expect(quotaWords(count(1204, 10_000), "en")).toEqual({
    words: "1,204 of 10,000 messages",
    detail: "Mail storage: 1,204 of 10,000 messages",
    warning: null,
  });
  expect(quotaWords(count(0, 1), "en").words).toBe("0 of 1 message");
});

test("the words follow the locale", () => {
  expect(quotaWords(octets(4.2 * GB, 10 * GB, true), "nl")).toEqual({
    words: "4,2 van 10 GB",
    detail: "Mailopslag: 4,2 GB van 10 GB gebruikt",
    warning: "Bijna vol",
  });
  expect(quotaWords(count(1204, 10_000), "nl").words).toBe("1.204 van 10.000 berichten");
});
