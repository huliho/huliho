// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use } from "react";
import type { ReactElement } from "react";

import type { Chord } from "../commands/keys";
import { prefetchMenus } from "./menus-chunk";

interface HintedButtonProps {
  label: string;
  // The keys of the button's command; none for one the palette alone runs.
  keys: readonly Chord[];
  align: "start" | "end";
  button: ReactElement;
}

function Hinted({ label, keys, align, button }: HintedButtonProps) {
  const { Tooltip } = use(prefetchMenus());
  return (
    <Tooltip label={label} keys={keys} align={align}>
      {button}
    </Tooltip>
  );
}

// An icon button of the shell with its hint, which rides the menus chunk;
// the button stands bare until the chunk is in.
export function HintedButton(props: HintedButtonProps) {
  return (
    <Suspense fallback={props.button}>
      <Hinted {...props} />
    </Suspense>
  );
}
