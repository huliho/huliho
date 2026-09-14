// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  AuthMethod,
  Credential,
  CredentialKind,
  Provider,
  SignInProvider,
  TlsMode,
} from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// The IANA ports the manual form starts from: IMAP and SMTP over TLS
// and their STARTTLS siblings (RFC 8314 sections 3.2 and 3.3, RFC 6409
// section 3.1).
const IMAPS_PORT = 993;
const IMAP_PORT = 143;
const SMTPS_PORT = 465;
const SUBMISSION_PORT = 587;

// A port is sixteen bits (RFC 793 section 3.1).
export const PORT_MIN = 1;
export const PORT_MAX = 65_535;

// The providers whose password route takes an app password.
const APP_PASSWORD_PROVIDERS = new Set<Provider>(["gmail", "icloud", "yahoo"]);

// The port a service listens on by default under the encryption.
export function defaultPort(service: "imap" | "smtp", tls: TlsMode): number {
  if (service === "imap") {
    return tls === "implicit" ? IMAPS_PORT : IMAP_PORT;
  }
  return tls === "implicit" ? SMTPS_PORT : SUBMISSION_PORT;
}

// The provider's name; the mail domain for a server without a preset.
export function providerName(provider: Provider, address: string): string {
  switch (provider) {
    case "gmail":
      return "Gmail";
    case "microsoft":
      return "Microsoft";
    case "fastmail":
      return "Fastmail";
    case "icloud":
      return "iCloud";
    case "yahoo":
      return "Yahoo";
    default:
      return address.slice(address.indexOf("@") + 1);
  }
}

export function credentialLabel(kind: CredentialKind, locale: Locale): string {
  if (kind === "appPassword") {
    return m.account_app_password_label({}, { locale });
  }
  if (kind === "apiToken") {
    return m.account_api_token_label({}, { locale });
  }
  return m.account_password_label({}, { locale });
}

// Where the secret comes from, said above the credential field.
export function credentialHint(provider: Provider, locale: Locale): string | null {
  switch (provider) {
    case "gmail":
      return m.account_hint_gmail({}, { locale });
    case "icloud":
      return m.account_hint_icloud({}, { locale });
    case "yahoo":
      return m.account_hint_yahoo({}, { locale });
    case "fastmail":
      return m.account_hint_fastmail({}, { locale });
    default:
      return null;
  }
}

// The sentence of a consent-only step without its button: Microsoft
// says who registers the client, any other provider that none is set up.
export function oauthHint(provider: Provider, locale: Locale): string {
  return provider === "microsoft"
    ? m.account_hint_microsoft({}, { locale })
    : m.account_no_providers({}, { locale });
}

// The sign-in provider behind a preset; null where the preset has none.
export function signInProviderOf(provider: Provider): SignInProvider | null {
  if (provider === "gmail") {
    return "google";
  }
  return provider === "microsoft" ? "microsoft" : null;
}

// The preset the provider's accounts get.
export function mailProviderOf(signIn: SignInProvider): Provider {
  return signIn === "google" ? "gmail" : "microsoft";
}

export function signInName(signIn: SignInProvider): string {
  return signIn === "google" ? "Google" : "Microsoft";
}

// Microsoft has no password route, so a refused consent offers none.
export function passwordRoute(signIn: SignInProvider): boolean {
  return signIn === "google";
}

// The sentence for a refused credential, by what the field asked for.
export function wrongCredential(kind: CredentialKind, locale: Locale): string {
  return kind === "apiToken"
    ? m.account_error_credentials_token({}, { locale })
    : m.account_error_credentials_password({}, { locale });
}

// What a stored row signs in with, for the reconnect step.
export function credentialKindOf(provider: Provider, authMethod: AuthMethod): CredentialKind {
  if (authMethod === "bearer") {
    return "apiToken";
  }
  if (authMethod === "oauth2") {
    return "oauth";
  }
  return APP_PASSWORD_PROVIDERS.has(provider) ? "appPassword" : "password";
}

// The credential the request carries: a token signs in as a bearer,
// every password kind as a password.
export function credentialOf(kind: CredentialKind, secret: string): Credential {
  return kind === "apiToken"
    ? { kind: "bearer", token: secret }
    : { kind: "password", password: secret };
}
