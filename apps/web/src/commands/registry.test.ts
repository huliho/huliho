// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { dispatchKey, installCommandListener, registerCommand } from "./registry";

const cleanups: (() => void)[] = [];

function register(key: string): Mock<() => void> {
  const run = vi.fn<() => void>();
  cleanups.push(registerCommand({ id: `test.${key}`, key, run }));
  return run;
}

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, cancelable: true, bubbles: true, ...init });
}

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  document.body.innerHTML = "";
});

test("the latest command for a key runs and is unregistered in order", () => {
  const first = register("z");
  const second = vi.fn<() => void>();
  const unregister = registerCommand({ id: "later", key: "z", run: second });
  const event = keydown("z");
  dispatchKey(event);
  expect(second).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(true);
  unregister();
  dispatchKey(keydown("z"));
  expect(first).toHaveBeenCalledOnce();
});

test("nothing fires while typing or inside a dialog", () => {
  const run = register("z");
  cleanups.push(installCommandListener());
  for (const html of [
    "<input>",
    "<textarea></textarea>",
    "<select></select>",
    '<div contenteditable="true"></div>',
    '<div role="dialog"><button></button></div>',
  ]) {
    document.body.innerHTML = html;
    const target = document.body.querySelector(
      "input, textarea, select, div[contenteditable], button",
    );
    target?.dispatchEvent(keydown("z"));
  }
  expect(run).not.toHaveBeenCalled();
});

test("modifiers and handled events are left alone", () => {
  const run = register("z");
  dispatchKey(keydown("z", { ctrlKey: true }));
  dispatchKey(keydown("z", { metaKey: true }));
  dispatchKey(keydown("z", { altKey: true }));
  const handled = keydown("z");
  handled.preventDefault();
  dispatchKey(handled);
  expect(run).not.toHaveBeenCalled();
});

test("the window listener installs once and uninstalls", () => {
  const run = register("Escape");
  const uninstall = installCommandListener();
  window.dispatchEvent(keydown("Escape"));
  expect(run).toHaveBeenCalledOnce();
  uninstall();
  window.dispatchEvent(keydown("Escape"));
  expect(run).toHaveBeenCalledOnce();
});
