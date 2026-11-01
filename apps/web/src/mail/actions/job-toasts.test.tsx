// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress } from "@huliho/core";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { ToastProvider, Toasts, toastManager } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { showJobEnd } from "./job-toasts";
import { endSentence } from "./job-words";
import { boxOf } from "./writable-rig";

// The menus chunk, refused while `refusal.on` holds, as an import is
// offline or once a deploy has replaced the file.
const refusal = vi.hoisted(() => ({ on: false }));
vi.mock("../menus-chunk", async (importActual) => {
  const actual = await importActual<typeof import("../menus-chunk")>();
  return {
    prefetchMenus: () =>
      refusal.on ? Promise.reject(new Error("refused")) : actual.prefetchMenus(),
  };
});

const EMPTY = { kind: "empty", mailboxId: "mb-trash" } as const;

function ended(state: JobProgress["state"], done: number, total: number | null): JobProgress {
  return { job: EMPTY, done, total, last: null, state };
}

afterEach(() => {
  act(() => {
    toastManager.close();
  });
  cleanup();
  vi.restoreAllMocks();
  refusal.on = false;
});

test("an end says how the job went in the role's words or the mailbox's name", () => {
  const trash = boxOf("trash");
  const junk = boxOf("junk");
  const facturen = { ...boxOf("inbox"), name: "Facturen", role: null };
  const read = (progress: JobProgress) => ({
    ...progress,
    job: { kind: "markRead" as const, mailboxId: facturen.id },
  });
  expect(endSentence(ended("done", 1204, 1204), trash, "en")).toBe("Trash emptied.");
  expect(endSentence(ended("done", 9, 9), junk, "nl")).toBe("Spam leeggemaakt.");
  expect(endSentence(read(ended("done", 4, 4)), facturen, "en")).toBe("Facturen marked as read.");
  expect(endSentence(ended("canceled", 500, 1204), trash, "en")).toBe(
    "Stopped after 500 of 1,204.",
  );
  expect(endSentence(ended("failed", 500, 1204), trash, "en")).toBe(
    "Couldn’t empty Trash. 500 of 1,204 were deleted.",
  );
  expect(endSentence(ended("failed", 1, 1204), trash, "nl")).toBe(
    "Prullenbak leegmaken is niet gelukt. 1 van 1.204 is verwijderd.",
  );
  expect(endSentence(ended("failed", 0, null), trash, "en")).toBe(
    "Couldn’t empty Trash. Nothing was deleted.",
  );
  expect(endSentence(read(ended("failed", 0, null)), facturen, "en")).toBe(
    "Couldn’t mark Facturen as read. Nothing was marked.",
  );
});

test("a server that leaves the total out has the count reached stand in for it", () => {
  expect(endSentence(ended("canceled", 500, null), boxOf("trash"), "en")).toBe(
    "Stopped after 500 of 500.",
  );
});

test("a failed job's toast carries Try again, which asks for the same job; a done one carries nothing", async () => {
  const cache = fixtureCache({});
  render(
    <ToastProvider>
      <Toasts />
    </ToastProvider>,
  );
  const trash = boxOf("trash");
  await act(async () => {
    await showJobEnd({
      cache,
      accountId: "a1",
      progress: ended("done", 12, 12),
      mailbox: trash,
      locale: "en",
    });
  });
  await screen.findByText("Trash emptied.");
  expect(screen.queryByRole("button")).toBeNull();
  await act(async () => {
    await showJobEnd({
      cache,
      accountId: "a1",
      progress: ended("failed", 0, 12),
      mailbox: trash,
      locale: "en",
    });
  });
  await screen.findByText("Couldn’t empty Trash. Nothing was deleted.");
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(cache.jobs).toEqual([EMPTY]);
});

test("an end whose words do not load says the pane's sentence in the danger tone, in the job's locale", async () => {
  refusal.on = true;
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  vi.spyOn(toastManager, "add");
  await showJobEnd({
    cache: fixtureCache({}),
    accountId: "a1",
    progress: ended("done", 12, 12),
    mailbox: boxOf("trash"),
    locale: "nl",
  });
  expect(toastManager.add).toHaveBeenCalledExactlyOnceWith({
    description:
      "Dit deel van het scherm kon niet worden getoond. Je mail is veilig; er ging niets verloren.",
    data: { tone: "danger" },
  });
  expect(logged.mock.calls).toEqual([["mail: a job's words did not load", "refused"]]);
});
