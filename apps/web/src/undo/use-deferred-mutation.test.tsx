// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { installCommandListener } from "../commands/registry";
import { ToastProvider, Toasts, UNDO_WINDOW_MS } from "../design-system/toast";
import { flushPendingOnPageHide, reapplyPendingAfterFetch } from "./pending";
import { useDeferredMutation } from "./use-deferred-mutation";

interface Row {
  id: string;
}

const ROWS_KEY = ["rows"];
const ROWS: Row[] = [{ id: "a" }, { id: "b" }];
// Margin past a timer, so its callback has certainly run.
const SETTLE_MS = 1_000;
// Past the undo window, so a timer that should have fired has fired.
const PAST_WINDOW_MS = UNDO_WINDOW_MS + SETTLE_MS;
// Inside the window, so a second removal overlaps the first.
const HALF_WINDOW_MS = UNDO_WINDOW_MS / 2;

type Mutate = (id: string, options: { keepalive: boolean }) => Promise<void>;

// The server keeps answering with both rows, as it does until a removal is sent.
const server = vi.fn<() => Promise<Row[]>>(() => Promise.resolve(ROWS));

function Harness({ mutate }: { mutate: Mutate }) {
  const query = useQuery({ queryKey: ROWS_KEY, queryFn: server, staleTime: Infinity });
  const remove = useDeferredMutation<Row, string>({
    queryKey: ROWS_KEY,
    keep: (row, id) => row.id !== id,
    mutate,
    message: (removed) => `${removed.map((row) => row.id).join(",")} gone`,
    failureMessage: "Could not remove the row.",
  });
  return (
    <>
      <output data-testid="ids">{(query.data ?? []).map((row) => row.id).join(",")}</output>
      {ROWS.map((row) => (
        <button
          key={row.id}
          type="button"
          onClick={() => {
            remove(row.id);
          }}
        >
          remove {row.id}
        </button>
      ))}
    </>
  );
}

let client: QueryClient;
let cleanups: (() => void)[] = [];

function renderHarness(mutate: Mutate): void {
  client = new QueryClient();
  client.setQueryData(ROWS_KEY, ROWS);
  cleanups = [installCommandListener(), flushPendingOnPageHide(), reapplyPendingAfterFetch(client)];
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <Harness mutate={mutate} />
        <Toasts />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

function ids(): string {
  return screen.getByTestId("ids").textContent;
}

// The query cache tells its observers on a timer, so the clock moves a tick.
async function elapse(ms: number): Promise<void> {
  await act(() => vi.advanceTimersByTimeAsync(ms));
}

async function removeRow(id: string): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: `remove ${id}` }));
  await elapse(0);
}

async function undoByKey(): Promise<void> {
  fireEvent.keyDown(window, { key: "z" });
  await elapse(0);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  for (const cleanupListener of cleanups) {
    cleanupListener();
  }
  cleanup();
  vi.useRealTimers();
});

test("the row leaves at once and undo brings it back without a request", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  expect(ids()).toBe("a");
  expect(screen.getByText("b gone")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  await elapse(0);
  expect(ids()).toBe("a,b");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).not.toHaveBeenCalled();
});

test("the z key undoes the latest removal", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await undoByKey();
  expect(ids()).toBe("a,b");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).not.toHaveBeenCalled();
});

test("when the toast runs out the mutation fires once", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).toHaveBeenCalledExactlyOnceWith("b", { keepalive: false });
  expect(screen.queryByText("b gone")).toBeNull();
  expect(ids()).toBe("a");
});

test("leaving the page flushes with keepalive and closes the toast", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await act(() => {
    window.dispatchEvent(new Event("pagehide"));
    return Promise.resolve();
  });
  expect(mutate).toHaveBeenCalledExactlyOnceWith("b", { keepalive: true });
  await elapse(HALF_WINDOW_MS);
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

test("fresh data inside the window keeps a pending row out", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await act(() => client.refetchQueries({ queryKey: ROWS_KEY }));
  await elapse(0);
  expect(ids()).toBe("a");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).toHaveBeenCalledExactlyOnceWith("b", { keepalive: false });
});

test("undoing one removal leaves another pending removal in place", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await elapse(HALF_WINDOW_MS);
  await removeRow("a");
  expect(ids()).toBe("");
  await undoByKey();
  expect(ids()).toBe("a");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).toHaveBeenCalledExactlyOnceWith("b", { keepalive: false });
});

test("a row that fresh data adds inside the window survives the undo", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  server.mockResolvedValueOnce([...ROWS, { id: "c" }]);
  await removeRow("b");
  await act(() => client.refetchQueries({ queryKey: ROWS_KEY }));
  await elapse(0);
  expect(ids()).toBe("a,c");
  await undoByKey();
  expect(ids()).toBe("a,b,c");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).not.toHaveBeenCalled();
});

test("a second z during the first toast's exit undoes the next pending removal", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  // jsdom has no Web Animations API, so an exit would end at once; one
  // animation that never finishes holds the closing toast in its exit.
  const heldExit = { finished: new Promise<never>(() => undefined) };
  Object.defineProperty(Element.prototype, "getAnimations", {
    configurable: true,
    value: () => [heldExit],
  });
  try {
    renderHarness(mutate);
    await removeRow("b");
    await elapse(HALF_WINDOW_MS);
    await removeRow("a");
    await undoByKey();
    expect(ids()).toBe("a");
    const exiting = screen.getByText("a gone").closest<HTMLElement>('[role="dialog"]');
    expect(exiting?.hasAttribute("data-ending-style")).toBe(true);
    await undoByKey();
    expect(ids()).toBe("a,b");
    await elapse(PAST_WINDOW_MS);
    expect(mutate).not.toHaveBeenCalled();
  } finally {
    Reflect.deleteProperty(Element.prototype, "getAnimations");
  }
});

test("undoing the older removal first keeps both rows once the newer one is undone too", async () => {
  const mutate = vi.fn<Mutate>().mockResolvedValue(undefined);
  renderHarness(mutate);
  await removeRow("b");
  await elapse(HALF_WINDOW_MS);
  await removeRow("a");
  const olderToast = screen.getByText("b gone").closest<HTMLElement>('[role="dialog"]');
  if (olderToast === null) {
    throw new Error("the older toast is not rendered");
  }
  fireEvent.click(within(olderToast).getByRole("button", { name: "Undo" }));
  await elapse(0);
  expect(ids()).toBe("b");
  await undoByKey();
  expect(ids()).toBe("a,b");
  await elapse(PAST_WINDOW_MS);
  expect(mutate).not.toHaveBeenCalled();
});

test("a failed removal brings its row back, says so and spares the other pending one", async () => {
  const mutate = vi
    .fn<Mutate>()
    .mockImplementation((id) =>
      id === "b" ? Promise.reject(new Error("down")) : Promise.resolve(),
    );
  renderHarness(mutate);
  await removeRow("b");
  await elapse(HALF_WINDOW_MS);
  await removeRow("a");
  await elapse(HALF_WINDOW_MS + SETTLE_MS);
  expect(mutate).toHaveBeenCalledExactlyOnceWith("b", { keepalive: false });
  expect(screen.getByText("Could not remove the row.")).toBeDefined();
  expect(ids()).toBe("b");
});
