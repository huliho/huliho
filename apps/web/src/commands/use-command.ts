// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useSyncExternalStore } from "react";

import { commandsSnapshot, registerCommand, subscribeCommands } from "./registry";
import type { Command } from "./registry";

// Registers a command for the life of the component; null registers nothing.
export function useCommand(command: Command | null): void {
  const id = command?.id;
  const label = command?.label;
  const group = command?.group;
  const keys = command?.keys;
  const run = command?.run;
  useEffect(() => {
    if (
      id === undefined ||
      label === undefined ||
      group === undefined ||
      keys === undefined ||
      run === undefined
    ) {
      return undefined;
    }
    return registerCommand({ id, label, group, keys, run });
  }, [id, label, group, keys, run]);
}

// Registers a set of commands together; a new set replaces the whole.
export function useCommands(commands: readonly Command[]): void {
  useEffect(() => {
    const unregister = commands.map((command) => registerCommand(command));
    return () => {
      for (const off of unregister) {
        off();
      }
    };
  }, [commands]);
}

// The registered commands, for a surface that renders them.
export function useRegistered(): readonly Command[] {
  return useSyncExternalStore(subscribeCommands, commandsSnapshot, commandsSnapshot);
}
