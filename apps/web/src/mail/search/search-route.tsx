// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use } from "react";
import type { ReactNode } from "react";

import { Button } from "../../design-system/button";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { useLocale } from "../../i18n/locale";
import { prefetchMenus } from "../menus-chunk";
import { LoadingRows, ThreadList } from "../thread-list";

// What the search draws that the page's start holds already, handed in
// so the chunk reaches none of the start's modules.
const PARTS = { Button, EmptyState, ErrorState, ThreadList };

function LoadedSearch({ loading }: { loading: ReactNode }) {
  const { SearchView } = use(prefetchMenus());
  return <SearchView parts={PARTS} loading={loading} />;
}

// The search route: the search rides the menus chunk, which the shell
// fetches once it stands, so `/` finds it in; the list's still rows
// stand in the pane until then.
export function SearchRoute() {
  const locale = useLocale();
  const loading = <LoadingRows locale={locale} />;
  return (
    <Suspense fallback={loading}>
      <LoadedSearch loading={loading} />
    </Suspense>
  );
}
