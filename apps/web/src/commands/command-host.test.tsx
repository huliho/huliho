// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { CommandHost, prefetchOverlays } from "./command-host";
import { COMMANDS } from "./fixtures";
import { dispatchKey, registerCommand } from "./registry";

// The overlays chunk, refused while `refusal.on` holds, as an import is
// offline or once a deploy has replaced the file.
const refusal = vi.hoisted(() => ({ on: false }));
vi.mock("./overlays", async (importActual) => {
  if (refusal.on) {
    throw new Error("refused");
  }
  return importActual<typeof import("./overlays")>();
});

// Long enough for the test runner to transform the overlays' chunk once.
const CHUNK_TIMEOUT_MS = 15_000;
const cleanups: (() => void)[] = [];

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, cancelable: true, ...init });
}

// The dialog's fade has ended, as Base UI tells the host once the transition finishes.
function fadeOut(): void {
  for (const popup of document.querySelectorAll('[role="dialog"]')) {
    fireEvent.transitionEnd(popup);
  }
}

// The popup's fade, held open the way a browser's transition holds it
// until the returned function ends it.
function holdFade(popup: Element): () => void {
  const fade = Promise.withResolvers<null>();
  Object.defineProperty(popup, "getAnimations", {
    configurable: true,
    value: () => [{ finished: fade.promise }],
  });
  return () => {
    fade.resolve(null);
  };
}

// The chunk in, as the host's prefetch leaves it before a key is pressed.
async function landed(): Promise<void> {
  await act(async () => {
    await prefetchOverlays();
  });
}

// A step whose render suspends is retried only when its act is awaited.
function awaited(step: () => void): Promise<void> {
  return act(() => {
    step();
    return Promise.resolve();
  });
}

// A jump registered for the palette to pick, with the palette open on its input.
async function paletteOverJump(run: () => void): Promise<HTMLElement> {
  cleanups.push(
    registerCommand({
      id: "go.junk",
      label: "Go to Junk",
      group: "go",
      keys: [{ key: "g" }, { key: "j" }],
      run,
    }),
  );
  await landed();
  render(<CommandHost />);
  act(() => {
    dispatchKey(keydown("k", { ctrlKey: true }));
  });
  return screen.findByRole("combobox", { name: "Command palette" });
}

beforeEach(() => {
  for (const command of COMMANDS.filter((entry) => entry.group === "go")) {
    cleanups.push(registerCommand(command));
  }
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  refusal.on = false;
  localStorage.clear();
  for (const off of cleanups.splice(0)) {
    off();
  }
});

test(
  "a refused chunk shows the error in a layer of its own; Escape closes it and Try again fetches the chunk again",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    refusal.on = true;
    const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const origin = document.createElement("button");
    document.body.append(origin);
    origin.focus();
    render(<CommandHost />);
    await awaited(() => {
      dispatchKey(keydown("?"));
    });
    const dialog = await screen.findByRole("dialog", { name: "Keyboard" });
    expect(within(dialog).getByRole("alert").textContent).toContain(
      "Couldn’t show this part of the screen.",
    );
    // The layer takes the focus in the effect after its mount.
    await vi.waitFor(() => {
      expect(dialog.contains(document.activeElement)).toBe(true);
    });
    expect(logged.mock.calls).toContainEqual(["pane: a render failed", "Error"]);
    fireEvent.keyDown(dialog, { key: "Escape" });
    fadeOut();
    await vi.waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(document.activeElement).toBe(origin);
    await awaited(() => {
      dispatchKey(keydown("?"));
    });
    const again = await screen.findByRole("dialog", { name: "Keyboard" });
    refusal.on = false;
    await awaited(() => {
      fireEvent.click(within(again).getByRole("button", { name: "Try again" }));
    });
    expect(document.activeElement).not.toBe(document.body);
    // The first import that lands transforms the chunk, which takes the runner a while.
    expect(await screen.findByText("Go to Inbox", {}, { timeout: CHUNK_TIMEOUT_MS })).toBeDefined();
    await vi.waitFor(() => {
      expect(
        screen.getByRole("dialog", { name: "Keyboard" }).contains(document.activeElement),
      ).toBe(true);
    });
    origin.remove();
  },
);

test(
  "the question mark opens the overlay over the registry and Escape closes it",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    await landed();
    render(<CommandHost />);
    act(() => {
      dispatchKey(keydown("?"));
    });
    const dialog = await screen.findByRole("dialog", { name: "Keyboard" });
    expect(screen.getByText("Go to Inbox")).toBeDefined();
    expect(screen.getByText("Command palette")).toBeDefined();
    fireEvent.keyDown(dialog, { key: "Escape" });
    fadeOut();
    await vi.waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  },
);

test(
  "a command picked in the palette runs once the palette has closed and is remembered as recent",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    const jumped = vi.fn<() => void>();
    const input = await paletteOverJump(jumped);
    fireEvent.change(input, { target: { value: "junk" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fadeOut();
    // The test runner has no transition, so the close completes at once.
    await vi.waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(jumped).toHaveBeenCalledOnce();
    expect(JSON.parse(localStorage.getItem("huliho-recent-commands") ?? "[]")).toEqual(["go.junk"]);
    act(() => {
      dispatchKey(keydown("k", { ctrlKey: true }));
    });
    await screen.findByRole("combobox", { name: "Command palette" });
    expect(screen.getAllByRole("option")[0]?.textContent).toBe("Go to Junkg j");
  },
);

test(
  "a key that opens a surface while a pick's palette still fades runs the pick first and never again",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    const jumped = vi.fn<() => void>();
    const input = await paletteOverJump(jumped);
    const endFade = holdFade(screen.getByRole("dialog", { name: "Command palette" }));
    fireEvent.change(input, { target: { value: "junk" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(jumped).not.toHaveBeenCalled();
    act(() => {
      dispatchKey(keydown("?"));
    });
    expect(jumped).toHaveBeenCalledOnce();
    const overlay = await screen.findByRole("dialog", { name: "Keyboard" });
    expect(screen.queryByRole("combobox")).toBeNull();
    await act(async () => {
      endFade();
      await Promise.resolve();
    });
    fireEvent.keyDown(overlay, { key: "Escape" });
    fadeOut();
    await vi.waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(jumped).toHaveBeenCalledOnce();
  },
);

test(
  "the overlay opens from the palette once the palette has closed",
  { timeout: CHUNK_TIMEOUT_MS },
  async () => {
    await landed();
    render(<CommandHost />);
    act(() => {
      dispatchKey(keydown("k", { ctrlKey: true }));
    });
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    fireEvent.change(input, { target: { value: "shortcuts" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fadeOut();
    expect(await screen.findByRole("dialog", { name: "Keyboard" })).toBeDefined();
    expect(screen.queryByRole("combobox")).toBeNull();
  },
);
