// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLayout } from "../shell/breakpoints";

// Whether the settings pages sit beside their navigation: from the tablet width on.
export function useSidebarShown(): boolean {
  return useLayout() !== "phone";
}
