// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { chunk } from "../shell/chunk";

// What `./account-menu` exports is one chunk outside the initial bundle,
// so the code those parts share stays out of the page's start. The shell
// fetches it on mount, so the first press and `/` find it in.
export const prefetchMenus = chunk(() => import("./account-menu"));
