// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CredentialError } from "@huliho/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { useCredentialMutation } from "./use-credential-mutation";

const RETRY_SECONDS = 3;
const SECOND_MS = 1_000;

type Check = () => Promise<void>;

function Harness({ check, onSuccess }: { check: Check; onSuccess: () => void }) {
  const mutation = useCredentialMutation<undefined>(check, { onSuccess });
  return (
    <>
      <button
        type="button"
        onClick={() => {
          mutation.mutate(undefined);
        }}
      >
        check
      </button>
      <output>{`${mutation.failure ?? "none"} ${String(mutation.retryRemaining)}`}</output>
    </>
  );
}

function renderHarness(check: Check, onSuccess: () => void = () => undefined): void {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Harness check={check} onSuccess={onSuccess} />
    </QueryClientProvider>,
  );
}

function press(): void {
  fireEvent.click(screen.getByRole("button", { name: "check" }));
}

beforeEach(() => {
  // Only the countdown is faked; the mutation settles on real microtasks.
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("a rate limited refusal holds the caller and lets go at zero", async () => {
  renderHarness(() => Promise.reject(new CredentialError("rate_limited", RETRY_SECONDS)));
  press();
  await screen.findByText("rate_limited 3");
  await act(() => vi.advanceTimersByTimeAsync(SECOND_MS));
  expect(screen.getByText("rate_limited 2")).toBeDefined();
  await act(() => vi.advanceTimersByTimeAsync(2 * SECOND_MS));
  expect(screen.getByText("none null")).toBeDefined();
});

test("other refusals carry their code and anything else reads as unavailable", async () => {
  renderHarness(() => Promise.reject(new CredentialError("invalid_credentials")));
  press();
  expect(await screen.findByText("invalid_credentials null")).toBeDefined();
  cleanup();
  renderHarness(() => Promise.reject(new Error("boom")));
  press();
  expect(await screen.findByText("unavailable null")).toBeDefined();
});

test("success runs the handler once", async () => {
  const onSuccess = vi.fn<() => void>();
  renderHarness(() => Promise.resolve(), onSuccess);
  press();
  await vi.waitFor(() => {
    expect(onSuccess).toHaveBeenCalledOnce();
  });
  expect(screen.getByText("none null")).toBeDefined();
});
