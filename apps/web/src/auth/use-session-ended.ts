// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useSignOut } from "./use-sign-out";

// A session gone mid-request ends here: a word about it, then sign-in.
export function useSessionEnded(locale: Locale): () => void {
  const signOut = useSignOut(locale);
  return () => {
    toastManager.add({ description: m.session_ended({}, { locale }) });
    signOut();
  };
}
