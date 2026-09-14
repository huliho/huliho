// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { expect, test } from "vitest";

import { captionOf } from "./caption";

const NOW = 1_778_750_400_000;
const MINUTES = 15;

const CONNECTED: AccountRow = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: NOW,
};
const EXPIRED: AccountRow = { ...CONNECTED, stoppedCause: "credentials", stoppedAt: NOW };
const STOPPED: AccountRow = { ...CONNECTED, stoppedCause: "connection", stoppedAt: NOW };

test("a connected row has no caption until a retry brought it back", () => {
  expect(captionOf(CONNECTED, undefined, MINUTES, "en")).toBeNull();
  expect(captionOf(CONNECTED, "pending", MINUTES, "en")).toBeNull();
  expect(captionOf(CONNECTED, "resumed", MINUTES, "en")).toEqual({
    text: "Connected again.",
    tone: "muted",
    live: "status",
  });
});

test("a stop names its cause; a retry that left it standing says so as an alert", () => {
  expect(captionOf(EXPIRED, undefined, MINUTES, "en")).toEqual({
    text: "Connection expired",
    tone: "warn",
    live: null,
  });
  expect(captionOf(EXPIRED, "stillStopped", MINUTES, "en")?.text).toBe("Connection expired");
  expect(captionOf(STOPPED, undefined, MINUTES, "en")).toEqual({
    text: "Couldn’t reach the server and stopped trying. The connection is checked again every 15 minutes.",
    tone: "warn",
    live: null,
  });
  expect(captionOf(STOPPED, "pending", MINUTES, "en")?.text).toContain("stopped trying");
  expect(captionOf(STOPPED, "stillStopped", MINUTES, "en")).toEqual({
    text: "Still couldn’t reach the server. The connection is checked again every 15 minutes.",
    tone: "danger",
    live: "alert",
  });
  expect(captionOf(STOPPED, undefined, 1, "en")?.text).toContain("checked again every minute.");
  expect(captionOf(STOPPED, undefined, MINUTES, "nl")?.text).toContain("elke 15 minuten");
});

test("a check that could not run says so on a stopped row; a row that moved on shows its own state", () => {
  expect(captionOf(STOPPED, "failed", MINUTES, "en")).toEqual({
    text: "Couldn’t check the connection. Try again in a moment.",
    tone: "danger",
    live: "alert",
  });
  expect(captionOf(CONNECTED, "failed", MINUTES, "en")).toBeNull();
  expect(captionOf(CONNECTED, "stillStopped", MINUTES, "en")).toBeNull();
  expect(captionOf(EXPIRED, "failed", MINUTES, "en")?.text).toBe("Connection expired");
});
