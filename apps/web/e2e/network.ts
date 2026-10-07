// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";

// The network as a device has it: connected, or with the browser
// saying offline and every request to the server failing, the mocked
// routes included.
export async function network(page: Page): Promise<(connected: boolean) => Promise<void>> {
  let online = true;
  await page
    .context()
    .route("**/api/**", (route) =>
      online ? route.fallback() : route.abort("internetdisconnected"),
    );
  return async (connected) => {
    online = connected;
    await page.context().setOffline(!connected);
  };
}
