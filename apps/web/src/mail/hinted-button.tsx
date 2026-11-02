// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useLayoutEffect, useRef } from "react";
import type { ReactElement, RefObject } from "react";

import type { Chord } from "../commands/keys";
import { prefetchMenus } from "./menus-chunk";

// A box around the button finds it and draws nothing of its own.
const BOX_STYLE = { display: "contents" };

interface HintedButtonProps {
  label: string;
  // The keys of the button's command; none for one the palette alone runs.
  keys: readonly Chord[];
  align: "start" | "end";
  button: ReactElement;
}

// What the bare button hands the hinted one: the focus it held as it left
// and a press it kept until then.
type Handed = RefObject<{ focus: boolean; press: boolean }>;

// A press waits for the hinted button, so whatever it opens opens from
// the element that stays and hands the focus back to it.
function Bare({ button, heldRef }: { button: ReactElement; heldRef: Handed }) {
  const box = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const around = box.current;
    const marks = heldRef.current;
    const keep = (event: Event): void => {
      event.stopPropagation();
      marks.press = true;
    };
    around?.addEventListener("click", keep, { capture: true });
    return () => {
      around?.removeEventListener("click", keep, { capture: true });
      marks.focus = around?.contains(document.activeElement) === true;
    };
  }, [heldRef]);
  return (
    <span ref={box} style={BOX_STYLE}>
      {button}
    </span>
  );
}

function Hinted({ label, keys, align, button, heldRef }: HintedButtonProps & { heldRef: Handed }) {
  const { Tooltip } = use(prefetchMenus());
  const box = useRef<HTMLSpanElement>(null);
  // A mark is spent as it is read, so a second run of the effect finds none.
  useLayoutEffect(() => {
    const hinted = box.current?.querySelector("button");
    const marks = heldRef.current;
    if (marks.focus) {
      marks.focus = false;
      hinted?.focus();
    }
    if (marks.press) {
      marks.press = false;
      hinted?.click();
    }
  }, [heldRef]);
  return (
    <span ref={box} style={BOX_STYLE}>
      <Tooltip label={label} keys={keys} align={align}>
        {button}
      </Tooltip>
    </span>
  );
}

// An icon button of the shell with its hint, which rides the menus chunk;
// the button stands bare until the chunk is in. The hinted one is a new
// element, so it takes over what the bare one held.
export function HintedButton(props: HintedButtonProps) {
  const heldRef = useRef({ focus: false, press: false });
  return (
    <Suspense fallback={<Bare button={props.button} heldRef={heldRef} />}>
      <Hinted {...props} heldRef={heldRef} />
    </Suspense>
  );
}
