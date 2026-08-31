// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export function cx(...classNames: (string | undefined)[]): string {
  return classNames.filter((name) => name !== undefined && name !== "").join(" ");
}
