// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { chunk } from "../shell/chunk";

// The account menu, the bulk bar, the mailbox menu, the quota bar, the
// search and the search button's hint are one chunk outside the initial
// bundle, so the code they share stays out of the page's start; the shell
// fetches it on mount, so the first press and `/` find it in.
export const prefetchMenus = chunk(() => import("./account-menu"));
