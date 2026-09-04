// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { deviceLabel, isUnknownDevice, revokedToast } from "./device-label";

const BASE = { browser: null, os: null, phone: false, installed: false };

test("labels name what the server knows and say when it knows nothing", () => {
  expect(deviceLabel({ ...BASE, browser: "Firefox", os: "Linux" }, "en")).toBe("Firefox on Linux");
  expect(deviceLabel({ ...BASE, browser: "Safari" }, "en")).toBe("Safari on unknown system");
  expect(deviceLabel({ ...BASE, os: "Windows" }, "en")).toBe("Unknown browser on Windows");
  expect(deviceLabel(BASE, "en")).toBe("Unknown device");
  expect(isUnknownDevice(BASE)).toBe(true);
  expect(isUnknownDevice({ ...BASE, installed: true })).toBe(false);
});

test("phones and installed apps read as such", () => {
  expect(
    deviceLabel({ browser: "Chrome", os: "Android", phone: true, installed: true }, "en"),
  ).toBe("Phone, installed app, Android");
  expect(deviceLabel({ browser: "Safari", os: "iOS", phone: true, installed: false }, "en")).toBe(
    "Phone, Safari on iOS",
  );
  expect(deviceLabel({ browser: "Edge", os: "Windows", phone: false, installed: true }, "en")).toBe(
    "Installed app, Windows",
  );
});

test("the toast names the phone, else the browser or system, else nothing", () => {
  expect(revokedToast({ ...BASE, browser: "Chrome", os: "Android", phone: true }, "en")).toBe(
    "Phone session revoked.",
  );
  expect(revokedToast({ ...BASE, browser: "Safari", os: "macOS" }, "en")).toBe(
    "Safari session revoked.",
  );
  expect(revokedToast({ ...BASE, os: "Linux" }, "nl")).toBe("Sessie van Linux ingetrokken.");
  expect(revokedToast(BASE, "en")).toBe("Session revoked.");
  expect(revokedToast(undefined, "nl")).toBe("Sessie ingetrokken.");
});
