// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, MailboxSetOutcome, MailboxWrite } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";

import { toastManager } from "../../design-system/toast";
import { fixtureCache } from "../../mail/fixture-cache";
import { SETTLE_MS, useFolderWrites } from "./use-folder-writes";

const ACCOUNT = "acc-1";
const WRITE: MailboxWrite = { update: { "mb-a": { role: null } } };

function outcome(notUpdated: MailboxSetOutcome["notUpdated"] = {}): MailboxSetOutcome {
  return {
    created: {},
    updated: [],
    destroyed: [],
    notCreated: {},
    notUpdated,
    notDestroyed: {},
    changes: { mailboxes: true, windows: [], threads: [] },
  };
}

function setup(setMailbox: MailCache["setMailbox"]) {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const add = vi.spyOn(toastManager, "add");
  const cache: MailCache = { ...fixtureCache({}), setMailbox };
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useFolderWrites(cache, ACCOUNT, "en"), { wrapper });
  return { hook, invalidate, add };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("a pick stands while its write is out, and until the tree is read again", async () => {
  const answer = Promise.withResolvers<MailboxSetOutcome>();
  const write = vi.fn<MailCache["setMailbox"]>(() => answer.promise);
  const { hook, invalidate, add } = setup(write);
  act(() => {
    hook.result.current.send("role:archive", "mb-b", WRITE);
  });
  expect(hook.result.current.pending).toEqual({ control: "role:archive", value: "mb-b" });
  expect(write).toHaveBeenCalledExactlyOnceWith(ACCOUNT, WRITE);
  await act(async () => {
    answer.resolve(outcome());
    await answer.promise;
  });
  await waitFor(() => {
    expect(hook.result.current.pending).toBeNull();
  });
  expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.mailboxes(ACCOUNT) });
  expect(add).not.toHaveBeenCalled();
});

test("a second pick while one is out sends nothing", () => {
  const write = vi.fn<MailCache["setMailbox"]>(
    () => Promise.withResolvers<MailboxSetOutcome>().promise,
  );
  const { hook } = setup(write);
  act(() => {
    hook.result.current.send("role:archive", "mb-b", WRITE);
  });
  act(() => {
    hook.result.current.send("role:sent", "mb-c", { update: { "mb-c": { role: "sent" } } });
  });
  expect(write).toHaveBeenCalledTimes(1);
  expect(hook.result.current.pending?.control).toBe("role:archive");
});

test("a write that lands says what it was given to say; a refused one says the server refused", async () => {
  const write = vi
    .fn<MailCache["setMailbox"]>()
    .mockResolvedValueOnce(outcome())
    .mockResolvedValueOnce(outcome({ "mb-a": { type: "forbidden", properties: [] } }));
  const { hook, add } = setup(write);
  act(() => {
    hook.result.current.send("show:mb-a", "mb-a", WRITE, "Folder shown.");
  });
  await waitFor(() => {
    expect(add).toHaveBeenCalledWith({ description: "Folder shown." });
  });
  await waitFor(() => {
    expect(hook.result.current.pending).toBeNull();
  });
  act(() => {
    hook.result.current.send("show:mb-a", "mb-a", WRITE, "Folder shown.");
  });
  await waitFor(() => {
    expect(add).toHaveBeenCalledWith({
      description: "The server refused. Try again.",
      data: { tone: "danger" },
    });
  });
  await waitFor(() => {
    expect(hook.result.current.pending).toBeNull();
  });
});

test("a write that never reached the server reads as refused and its cause stays in the console", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { hook, add } = setup(() => Promise.reject(new Error("gone")));
  act(() => {
    hook.result.current.send("role:archive", "mb-b", WRITE);
  });
  await waitFor(() => {
    expect(add).toHaveBeenCalledWith({
      description: "The server refused. Try again.",
      data: { tone: "danger" },
    });
  });
  expect(error).toHaveBeenCalledWith("settings: a folder write failed", "gone");
  await waitFor(() => {
    expect(hook.result.current.pending).toBeNull();
  });
});

test("a key walking a select sends the last pick once, after the select stands still", () => {
  vi.useFakeTimers();
  const write = vi.fn<MailCache["setMailbox"]>(
    () => Promise.withResolvers<MailboxSetOutcome>().promise,
  );
  const { hook } = setup(write);
  const final = { update: { "mb-c": { role: "archive" } } };
  act(() => {
    hook.result.current.settle("role:archive", "mb-b", WRITE);
  });
  act(() => {
    vi.advanceTimersByTime(SETTLE_MS - 1);
  });
  act(() => {
    hook.result.current.settle("role:archive", "mb-c", final);
  });
  expect(hook.result.current.pending).toEqual({ control: "role:archive", value: "mb-c" });
  act(() => {
    vi.advanceTimersByTime(SETTLE_MS - 1);
  });
  expect(write).not.toHaveBeenCalled();
  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(write).toHaveBeenCalledExactlyOnceWith(ACCOUNT, final);
  vi.useRealTimers();
});

test("a walk back to the holder sends nothing and lets the row go", () => {
  vi.useFakeTimers();
  const write = vi.fn<MailCache["setMailbox"]>();
  const { hook } = setup(write);
  act(() => {
    hook.result.current.settle("role:archive", "mb-b", WRITE);
  });
  act(() => {
    hook.result.current.settle("role:archive", "mb-a", null);
  });
  expect(hook.result.current.pending).toBeNull();
  act(() => {
    vi.advanceTimersByTime(SETTLE_MS);
  });
  expect(write).not.toHaveBeenCalled();
  vi.useRealTimers();
});

test("a pick on another row while one waits or a write is out stays as it was", () => {
  vi.useFakeTimers();
  const write = vi.fn<MailCache["setMailbox"]>(
    () => Promise.withResolvers<MailboxSetOutcome>().promise,
  );
  const { hook } = setup(write);
  const other = { update: { "mb-c": { role: "sent" } } };
  act(() => {
    hook.result.current.settle("role:archive", "mb-b", WRITE);
  });
  act(() => {
    hook.result.current.settle("role:sent", "mb-c", other);
  });
  expect(hook.result.current.pending?.control).toBe("role:archive");
  act(() => {
    vi.advanceTimersByTime(SETTLE_MS);
  });
  expect(write).toHaveBeenCalledExactlyOnceWith(ACCOUNT, WRITE);
  act(() => {
    hook.result.current.settle("role:archive", "mb-c", other);
  });
  expect(hook.result.current.pending?.value).toBe("mb-b");
  act(() => {
    vi.advanceTimersByTime(SETTLE_MS);
  });
  expect(write).toHaveBeenCalledTimes(1);
  vi.useRealTimers();
});

test("a pick still waiting goes out when the screen leaves", () => {
  vi.useFakeTimers();
  const write = vi.fn<MailCache["setMailbox"]>(
    () => Promise.withResolvers<MailboxSetOutcome>().promise,
  );
  const { hook } = setup(write);
  act(() => {
    hook.result.current.settle("role:archive", "mb-b", WRITE);
  });
  hook.unmount();
  expect(write).toHaveBeenCalledExactlyOnceWith(ACCOUNT, WRITE);
  vi.useRealTimers();
});

test("offline a write is refused at once and nothing is sent", async () => {
  vi.stubGlobal("navigator", { onLine: false });
  const write = vi.fn<MailCache["setMailbox"]>();
  const { hook, add } = setup(write);
  act(() => {
    hook.result.current.send("role:archive", "mb-b", WRITE);
  });
  await waitFor(() => {
    expect(add).toHaveBeenCalledWith({
      description: "Changes need a connection.",
      data: { tone: "danger" },
    });
  });
  expect(write).not.toHaveBeenCalled();
  await waitFor(() => {
    expect(hook.result.current.pending).toBeNull();
  });
});
