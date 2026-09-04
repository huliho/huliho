// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export interface Command {
  id: string;
  key: string;
  run: () => void;
}

const commands: Command[] = [];

// Typing and layered surfaces keep their keys to themselves.
const CLAIMED_BY_FOCUS =
  'input, textarea, select, [contenteditable], [role="dialog"], [role="alertdialog"]';

// Later registrations win, so a toast's undo outranks a page command.
export function registerCommand(command: Command): () => void {
  commands.push(command);
  return () => {
    const index = commands.lastIndexOf(command);
    if (index !== -1) {
      commands.splice(index, 1);
    }
  };
}

function claimedByFocus(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(CLAIMED_BY_FOCUS) !== null;
}

export function dispatchKey(event: KeyboardEvent): void {
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) {
    return;
  }
  if (claimedByFocus(event.target)) {
    return;
  }
  const command = commands.findLast((candidate) => candidate.key === event.key);
  if (command === undefined) {
    return;
  }
  event.preventDefault();
  command.run();
}

export function installCommandListener(target: Window = window): () => void {
  target.addEventListener("keydown", dispatchKey);
  return () => {
    target.removeEventListener("keydown", dispatchKey);
  };
}
