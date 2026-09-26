// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import type { Chord } from "./keys";
import {
  CHORD_TIMEOUT_MS,
  commandsSnapshot,
  dispatchKey,
  installCommandListener,
  registerCommand,
  subscribeCommands,
} from "./registry";

const cleanups: (() => void)[] = [];

function register(
  keys: readonly Chord[],
  id = keys.map((chord) => chord.key).join(""),
): Mock<() => void> {
  const run = vi.fn<() => void>();
  cleanups.push(registerCommand({ id: `test.${id}`, label: id, group: "app", keys, run }));
  return run;
}

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, cancelable: true, bubbles: true, ...init });
}

// A key pressed with the focus on `html`, so a claim by its ancestors is read.
function press(html: string, key: string): void {
  document.body.innerHTML = html;
  const target = document.body.querySelector("input, textarea, select, [contenteditable], button");
  target?.dispatchEvent(keydown(key));
}

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  document.body.innerHTML = "";
  vi.useRealTimers();
});

test("the latest command for a key runs and is unregistered in order", () => {
  const first = register([{ key: "z" }]);
  const second = vi.fn<() => void>();
  const unregister = registerCommand({
    id: "later",
    label: "later",
    group: "act",
    keys: [{ key: "z" }],
    run: second,
  });
  const event = keydown("z");
  dispatchKey(event);
  expect(second).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(true);
  unregister();
  dispatchKey(keydown("z"));
  expect(first).toHaveBeenCalledOnce();
});

test("a single key waits while typing; a combination fires there as well", () => {
  const single = register([{ key: "z" }]);
  const combined = register([{ key: "k", mod: true }]);
  cleanups.push(installCommandListener());
  for (const html of [
    "<input>",
    "<textarea></textarea>",
    "<select></select>",
    '<div contenteditable="true"></div>',
  ]) {
    press(html, "z");
    document.body.innerHTML = html;
    const target = document.body.querySelector("input, textarea, select, div");
    target?.dispatchEvent(keydown("k", { ctrlKey: true }));
  }
  expect(single).not.toHaveBeenCalled();
  expect(combined).toHaveBeenCalledTimes(4);
});

test("a dialog, a menu and a listbox keep every key to themselves", () => {
  const single = register([{ key: "z" }]);
  const combined = register([{ key: "k", mod: true }]);
  cleanups.push(installCommandListener());
  for (const role of ["dialog", "alertdialog", "menu", "listbox"]) {
    press(`<div role="${role}"><button></button></div>`, "z");
    document.body.querySelector("button")?.dispatchEvent(keydown("k", { ctrlKey: true }));
  }
  expect(single).not.toHaveBeenCalled();
  expect(combined).not.toHaveBeenCalled();
});

test("a handled event, Alt and the other platform's command key are left alone", () => {
  const single = register([{ key: "z" }]);
  const combined = register([{ key: "z", mod: true }]);
  dispatchKey(keydown("z", { altKey: true }));
  dispatchKey(keydown("z", { metaKey: true }));
  dispatchKey(keydown("z", { ctrlKey: true, metaKey: true }));
  const handled = keydown("z");
  handled.preventDefault();
  dispatchKey(handled);
  expect(single).not.toHaveBeenCalled();
  expect(combined).not.toHaveBeenCalled();
});

test("a combination matches its modifiers and its letter whatever the case", () => {
  const plain = register([{ key: "k", mod: true }]);
  const shifted = register([{ key: "l", mod: true, shift: true }]);
  dispatchKey(keydown("K", { ctrlKey: true }));
  dispatchKey(keydown("L", { ctrlKey: true, shiftKey: true }));
  dispatchKey(keydown("l", { ctrlKey: true }));
  dispatchKey(keydown("k", { ctrlKey: true, shiftKey: true }));
  expect(plain).toHaveBeenCalledOnce();
  expect(shifted).toHaveBeenCalledOnce();
});

test("a question mark fires as itself, however Shift got there", () => {
  const run = register([{ key: "?" }]);
  dispatchKey(keydown("?", { shiftKey: true }));
  expect(run).toHaveBeenCalledOnce();
});

test("a prefix waits for its letter and runs the sequence", () => {
  vi.useFakeTimers();
  const inbox = register([{ key: "g" }, { key: "i" }]);
  const gee = register([{ key: "g" }], "plain-g");
  const eye = register([{ key: "i" }], "plain-i");
  const opening = keydown("g");
  dispatchKey(opening);
  expect(opening.defaultPrevented).toBe(true);
  expect(gee).not.toHaveBeenCalled();
  vi.advanceTimersByTime(CHORD_TIMEOUT_MS - 1);
  dispatchKey(keydown("i"));
  expect(inbox).toHaveBeenCalledOnce();
  expect(eye).not.toHaveBeenCalled();
});

test("a prefix runs out, and a letter that completes nothing runs nothing of its own", () => {
  vi.useFakeTimers();
  const inbox = register([{ key: "g" }, { key: "i" }]);
  const eye = register([{ key: "i" }], "plain-i");
  dispatchKey(keydown("g"));
  vi.advanceTimersByTime(CHORD_TIMEOUT_MS);
  dispatchKey(keydown("i"));
  expect(inbox).not.toHaveBeenCalled();
  expect(eye).toHaveBeenCalledOnce();
  dispatchKey(keydown("g"));
  const stray = keydown("x");
  dispatchKey(stray);
  expect(stray.defaultPrevented).toBe(false);
  dispatchKey(keydown("i"));
  expect(inbox).not.toHaveBeenCalled();
  expect(eye).toHaveBeenCalledTimes(2);
});

test("a combination after a prefix cancels it and runs as usual", () => {
  const inbox = register([{ key: "g" }, { key: "i" }]);
  const palette = register([{ key: "k", mod: true }]);
  const eye = register([{ key: "i" }], "plain-i");
  dispatchKey(keydown("g"));
  dispatchKey(keydown("k", { ctrlKey: true }));
  dispatchKey(keydown("i"));
  expect(palette).toHaveBeenCalledOnce();
  expect(inbox).not.toHaveBeenCalled();
  expect(eye).toHaveBeenCalledOnce();
});

test("a key that opens no registered sequence runs on its own", () => {
  const gee = register([{ key: "g" }], "plain-g");
  dispatchKey(keydown("g"));
  expect(gee).toHaveBeenCalledOnce();
});

test("the window listener installs once, uninstalls and drops a pending prefix", () => {
  const inbox = register([{ key: "g" }, { key: "i" }]);
  const run = register([{ key: "Escape" }]);
  const uninstall = installCommandListener();
  window.dispatchEvent(keydown("Escape"));
  expect(run).toHaveBeenCalledOnce();
  window.dispatchEvent(keydown("g"));
  uninstall();
  window.dispatchEvent(keydown("Escape"));
  expect(run).toHaveBeenCalledOnce();
  dispatchKey(keydown("i"));
  expect(inbox).not.toHaveBeenCalled();
});

test("a surface reading the registry sees every change in order", () => {
  const seen: number[] = [];
  const unsubscribe = subscribeCommands(() => {
    seen.push(commandsSnapshot().length);
  });
  const before = commandsSnapshot();
  const off = registerCommand({
    id: "one",
    label: "One",
    group: "app",
    keys: [],
    run: vi.fn<() => void>(),
  });
  expect(commandsSnapshot()).not.toBe(before);
  expect(commandsSnapshot().map((command) => command.id)).toContain("one");
  off();
  expect(commandsSnapshot().map((command) => command.id)).not.toContain("one");
  expect(seen).toEqual([before.length + 1, before.length]);
  unsubscribe();
});
