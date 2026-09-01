// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { SignInError, signIn } from "@huliho/core";
import { sessionQueryOptions } from "@huliho/state";
import { BrandMark } from "../design-system/brand-mark";
import { LegalNotices } from "../legal/legal-notices";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import { SignInForm } from "./sign-in-form";
import styles from "./sign-in.module.css";

const COUNTDOWN_TICK_MS = 1_000;

export type SignInFailure = "invalid_credentials" | "rate_limited" | "unavailable" | null;

function failureOf(error: unknown): SignInFailure {
  if (error === null) {
    return null;
  }
  return error instanceof SignInError ? error.code : "unavailable";
}

function useTick(active: boolean, onTick: () => void): void {
  useEffect(() => {
    if (!active) {
      return undefined;
    }
    const timer = setInterval(onTick, COUNTDOWN_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [active, onTick]);
}

export function SignIn() {
  const locale = getLocale();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [retryRemaining, setRetryRemaining] = useState<number | null>(null);
  const mutation = useMutation({
    mutationFn: (input: { login: string; password: string }) => signIn(input.login, input.password),
    onSuccess: async () => {
      queryClient.removeQueries({ queryKey: sessionQueryOptions.queryKey });
      await navigate({ to: "/" });
    },
    onError: (error) => {
      if (error instanceof SignInError && error.code === "rate_limited") {
        setRetryRemaining(error.retryAfterSeconds);
      }
    },
  });

  useTick(retryRemaining !== null, () => {
    if (retryRemaining !== null && retryRemaining <= 1) {
      setRetryRemaining(null);
      mutation.reset();
    } else if (retryRemaining !== null) {
      setRetryRemaining(retryRemaining - 1);
    }
  });

  return (
    <main className={styles.screen}>
      <div className={styles.column}>
        <BrandMark heading />
        <SignInForm
          locale={locale}
          pending={mutation.isPending}
          failure={failureOf(mutation.error)}
          retryRemaining={retryRemaining}
          onSubmit={(input) => {
            mutation.mutate(input);
          }}
        />
        <p className={styles.adminNote}>{m.signin_admin_note({}, { locale })}</p>
        <LegalNotices locale={locale} />
      </div>
    </main>
  );
}
