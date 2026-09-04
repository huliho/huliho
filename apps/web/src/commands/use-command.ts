// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect } from "react";

import { registerCommand } from "./registry";
import type { Command } from "./registry";

// Registers a command for the life of the component; null registers nothing.
export function useCommand(command: Command | null): void {
  const id = command?.id;
  const key = command?.key;
  const run = command?.run;
  useEffect(() => {
    if (id === undefined || key === undefined || run === undefined) {
      return undefined;
    }
    return registerCommand({ id, key, run });
  }, [id, key, run]);
}
