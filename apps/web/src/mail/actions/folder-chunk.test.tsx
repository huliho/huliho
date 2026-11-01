// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { toastManager } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { FOLDER_LIMITS } from "../fixtures";
import { askFolder, closeFolderAsk, settleFolderAsk } from "./folder-asks";
import type { FolderAction, NameAsk } from "./folder-asks";
import { FolderHost } from "./folder-host";
import { folderItems } from "./folder-items";
import { MANAGED, managedOf } from "./writable-rig";

// The menus chunk through the product's own loader, refused while
// `refusal.on` holds, as an import is offline or once a deploy has
// replaced the file.
const refusal = vi.hoisted(() => ({ on: false }));
vi.mock("../menus-chunk", async () => {
  const { chunk } = await import("../../shell/chunk");
  return {
    prefetchMenus: chunk(async () => {
      if (refusal.on) {
        throw new Error("refused");
      }
      return import("../account-menu");
    }),
  };
});

const PANE_SENTENCE = "Couldn’t show this part of the screen. Your mail is safe; nothing was lost.";

// Presses one item of a managed folder's menu.
function press(action: FolderAction): void {
  const item = folderItems({
    locale: "en",
    cache: fixtureCache({}),
    accountId: "acc-1",
    mailbox: managedOf("mb-facturen"),
    mailboxes: MANAGED,
    limits: FOLDER_LIMITS,
    shownId: undefined,
    leave: () => undefined,
  }).find((entry) => entry.id === action);
  if (item === undefined) {
    throw new Error(`the folder offers no ${action}`);
  }
  item.run();
}

function newFolderAsk(): NameAsk {
  return {
    kind: "create",
    name: "",
    place: null,
    taken: [],
    limit: FOLDER_LIMITS.maxSizeMailboxName,
    write: () => Promise.resolve(null),
  };
}

afterEach(() => {
  act(() => {
    closeFolderAsk();
    settleFolderAsk();
    toastManager.close();
  });
  cleanup();
  vi.restoreAllMocks();
  refusal.on = false;
});

test("a pressed item whose chunk does not load says the pane's sentence in the danger tone", async () => {
  refusal.on = true;
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  vi.spyOn(toastManager, "add");
  press("subfolder");
  await vi.waitFor(() => {
    expect(toastManager.add).toHaveBeenCalledExactlyOnceWith({
      description: PANE_SENTENCE,
      data: { tone: "danger" },
    });
  });
  expect(logged.mock.calls).toEqual([["mail: a folder action did not load", "refused"]]);
});

test("a dialog whose chunk does not load shows the pane's sentence under its own name; Try again fetches the chunk afresh", async () => {
  // The chunk's module is in before Try again asks for it.
  await import("../account-menu");
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  render(<FolderHost locale="en" />);
  refusal.on = true;
  await act(async () => {
    askFolder(newFolderAsk());
    await Promise.resolve();
  });
  const dialog = await screen.findByRole("dialog", { name: "New folder" });
  expect(within(dialog).getByRole("alert").textContent).toContain(PANE_SENTENCE);
  refusal.on = false;
  await act(async () => {
    fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
    await Promise.resolve();
  });
  expect(await screen.findByRole("textbox", { name: "Folder name" })).toBeDefined();
});
