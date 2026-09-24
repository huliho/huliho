// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";
import type { RefObject } from "react";

import { useStatusText } from "./status-text";

// New mail is announced at most this often.
const ANNOUNCE_INTERVAL_MS = 30_000;

// The marker against the count it stands for: a scroll or an action in
// the list puts it away until a refresh finds more.
interface Marker {
  pending: number;
  dismissed: boolean;
}

export interface ListMarker {
  shown: boolean;
  dismiss: () => void;
}

// Whether the marker shows for `pending` new messages, and the word
// that goes to the polite region while it does: at most once per
// interval, and the region stands empty while the marker is away.
export function useMarker(
  pending: number,
  regionRef: RefObject<HTMLElement | null>,
  announcement: string,
): ListMarker {
  const [marker, setMarker] = useState<Marker>({ pending, dismissed: false });
  if (marker.pending !== pending) {
    setMarker({ pending, dismissed: false });
  }
  const shown = pending > 0 && !marker.dismissed;
  useStatusText(regionRef, shown ? announcement : "", ANNOUNCE_INTERVAL_MS);
  return {
    shown,
    dismiss: () => {
      if (shown) {
        setMarker({ pending, dismissed: true });
      }
    },
  };
}
