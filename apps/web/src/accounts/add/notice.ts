// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountsFailureCode } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { Step } from "./flow";

// The server a refusal names; null on a reconnect, where the row has no
// server name to check.
function hostOf(step: Step, manualServer: string): string | null {
  if (step.name === "found" || step.name === "confirmHost") {
    return step.found.host;
  }
  if (step.name === "reconnect" || (step.name === "connecting" && step.from.name === "reconnect")) {
    return null;
  }
  return step.name === "connecting" ? step.host : manualServer.trim();
}

// The row a reconnect refusal names, when the step is one.
function rowNameOf(step: Step): string {
  if (step.name === "reconnect") {
    return step.account.name;
  }
  return step.name === "connecting" && step.from.name === "reconnect" ? step.from.account.name : "";
}

// A server that did not answer as one: the host to check where there is
// one, the row's name where there is none.
function serverText(
  failure: AccountsFailureCode,
  host: string | null,
  name: string,
  locale: Locale,
): string {
  if (host === null) {
    return m.account_error_server_again({ name }, { locale });
  }
  return failure === "upstream_unreachable"
    ? m.account_error_unreachable({ host }, { locale })
    : m.account_error_unsupported({ host }, { locale });
}

// The refusals no field owns, said above the fields.
export function noticeText(
  failure: AccountsFailureCode | null,
  step: Step,
  manualServer: string,
  locale: Locale,
): string | null {
  switch (failure) {
    case "rate_limited":
      return m.account_error_rate_limited({}, { locale });
    case "unavailable":
      return m.signin_error_unavailable({}, { locale });
    case "upstream_unreachable":
    case "upstream_unsupported":
      return serverText(failure, hostOf(step, manualServer), rowNameOf(step), locale);
    case "smtp_auth_unavailable":
      return m.account_error_smtp_auth({}, { locale });
    case "invalid_request":
      return m.account_error_invalid({}, { locale });
    case "not_found":
      return m.account_error_gone({}, { locale });
    case "provider_not_configured":
      return m.account_no_providers({}, { locale });
    default:
      return null;
  }
}
