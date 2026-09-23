// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Preferences } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { ToastProvider, Toasts } from "../../design-system/toast";
import { usePreference } from "./use-preference";

const sessionEnded = vi.fn<() => void>();
vi.mock("../../auth/use-session-ended", () => ({
  useSessionEnded: () => sessionEnded,
}));

const SERVER: Preferences = { theme: "light" };
const serverAnswer = vi.fn<() => Promise<Preferences>>(() => Promise.resolve(SERVER));

function Harness() {
  const query = useQuery({
    queryKey: queryKeys.preferences,
    queryFn: serverAnswer,
    staleTime: Number.POSITIVE_INFINITY,
  });
  const change = usePreference("en");
  return (
    <>
      <output data-testid="theme">{query.data?.theme ?? "none"}</output>
      <button
        type="button"
        onClick={() => {
          change({ key: "theme", value: "dark" });
        }}
      >
        dark
      </button>
    </>
  );
}

async function renderHarness(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <Harness />
        <Toasts />
      </ToastProvider>
    </QueryClientProvider>,
  );
  await screen.findByText("light");
  return queryClient;
}

function answer(status: number): void {
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status })));
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  sessionEnded.mockReset();
});

test("the choice shows before the server answers", async () => {
  const { promise, resolve } = Promise.withResolvers<Response>();
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockReturnValue(promise));
  await renderHarness();
  fireEvent.click(screen.getByRole("button", { name: "dark" }));
  expect(await screen.findByText("dark")).toBeDefined();
  await act(async () => {
    resolve(new Response(null, { status: 204 }));
    await promise;
  });
  expect(screen.getByTestId("theme").textContent).toBe("dark");
});

test("a choice made while a refetch is in flight outlives the refetch's answer", async () => {
  answer(204);
  const queryClient = await renderHarness();
  const refetchAnswer = Promise.withResolvers<Preferences>();
  serverAnswer.mockReturnValueOnce(refetchAnswer.promise);
  const refetched = queryClient.refetchQueries({ queryKey: queryKeys.preferences });
  fireEvent.click(screen.getByRole("button", { name: "dark" }));
  expect(await screen.findByText("dark")).toBeDefined();
  await act(async () => {
    refetchAnswer.resolve(SERVER);
    await refetched;
  });
  expect(queryClient.getQueryData<Preferences>(queryKeys.preferences)).toEqual({ theme: "dark" });
  expect(screen.getByTestId("theme").textContent).toBe("dark");
});

test("a refused save puts the server's word back and says so", async () => {
  answer(500);
  await renderHarness();
  fireEvent.click(screen.getByRole("button", { name: "dark" }));
  expect(await screen.findByText("dark")).toBeDefined();
  expect(await screen.findByText("Couldn’t save that setting. Try again.")).toBeDefined();
  expect(await screen.findByText("light")).toBeDefined();
  expect(sessionEnded).not.toHaveBeenCalled();
});

test("a session that ended ends it here too", async () => {
  answer(401);
  await renderHarness();
  fireEvent.click(screen.getByRole("button", { name: "dark" }));
  await vi.waitFor(() => {
    expect(sessionEnded).toHaveBeenCalledOnce();
  });
  expect(screen.queryByText("Couldn’t save that setting. Try again.")).toBeNull();
});
