// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import {
  credentialHint,
  credentialKindOf,
  credentialLabel,
  credentialOf,
  mailProviderOf,
  oauthHint,
  passwordRoute,
  providerName,
  signInName,
  signInProviderOf,
  wrongCredential,
} from "./presets";

test("a preset names its provider; a generic server names the mail domain", () => {
  expect(providerName("fastmail", "sanne@fastmail.com")).toBe("Fastmail");
  expect(providerName("icloud", "sanne@me.com")).toBe("iCloud");
  expect(providerName("generic", "sanne@dekker-mail.nl")).toBe("dekker-mail.nl");
});

test("the label and the refusal follow the credential kind", () => {
  expect(credentialLabel("password", "en")).toBe("Password");
  expect(credentialLabel("appPassword", "en")).toBe("App password");
  expect(credentialLabel("apiToken", "en")).toBe("API token");
  expect(wrongCredential("apiToken", "en")).toContain("the token");
  expect(wrongCredential("appPassword", "en")).toContain("the password");
  expect(wrongCredential("password", "nl")).toContain("het wachtwoord");
});

test("every preset but generic says where its secret comes from", () => {
  expect(credentialHint("gmail", "en")).toContain("app password");
  expect(credentialHint("icloud", "en")).toContain("app-specific password");
  expect(credentialHint("yahoo", "en")).toContain("app password");
  expect(credentialHint("fastmail", "en")).toContain("API token");
  expect(credentialHint("microsoft", "en")).toBeNull();
  expect(credentialHint("generic", "en")).toBeNull();
});

test("a consent-only step without its button says who sets it up", () => {
  expect(oauthHint("microsoft", "en")).toContain("Microsoft Entra");
  expect(oauthHint("gmail", "en")).toContain("not set up here");
});

test("the sign-in provider behind a preset and the preset behind a sign-in provider", () => {
  expect(signInProviderOf("gmail")).toBe("google");
  expect(signInProviderOf("microsoft")).toBe("microsoft");
  for (const provider of ["fastmail", "icloud", "yahoo", "generic"] as const) {
    expect(signInProviderOf(provider)).toBeNull();
  }
  expect(mailProviderOf("google")).toBe("gmail");
  expect(mailProviderOf("microsoft")).toBe("microsoft");
  expect(signInName("google")).toBe("Google");
  expect(signInName("microsoft")).toBe("Microsoft");
  expect(passwordRoute("google")).toBe(true);
  expect(passwordRoute("microsoft")).toBe(false);
});

test("a stored row's kind follows its auth method, then its provider", () => {
  expect(credentialKindOf("fastmail", "bearer")).toBe("apiToken");
  expect(credentialKindOf("gmail", "oauth2")).toBe("oauth");
  expect(credentialKindOf("gmail", "password")).toBe("appPassword");
  expect(credentialKindOf("yahoo", "password")).toBe("appPassword");
  expect(credentialKindOf("generic", "password")).toBe("password");
});

test("a token goes out as a bearer, every password kind as a password", () => {
  expect(credentialOf("apiToken", "fmu1-x")).toEqual({ kind: "bearer", token: "fmu1-x" });
  expect(credentialOf("appPassword", "abcd")).toEqual({ kind: "password", password: "abcd" });
  expect(credentialOf("password", "abcd")).toEqual({ kind: "password", password: "abcd" });
});
