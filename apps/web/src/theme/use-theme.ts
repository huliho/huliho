// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

// The theme a page renders in, once the system's scheme stood in for no choice.
export type Theme = "light" | "dark";

// The system's scheme, which the colors follow while no theme is chosen.
export const DARK_QUERY = "(prefers-color-scheme: dark)";

function subscribe(onChange: () => void): () => void {
  const list = matchMedia(DARK_QUERY);
  list.addEventListener("change", onChange);
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => {
    list.removeEventListener("change", onChange);
    observer.disconnect();
  };
}

// The theme the app renders now: the one the document names, else the
// one the system prefers.
function renderedTheme(): Theme {
  const chosen = document.documentElement.dataset["theme"];
  if (chosen === "light" || chosen === "dark") {
    return chosen;
  }
  return matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

export function useTheme(): Theme {
  return useSyncExternalStore(subscribe, renderedTheme, () => "light");
}
