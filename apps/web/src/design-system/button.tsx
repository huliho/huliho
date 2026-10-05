// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Button as BaseButton } from "@base-ui/react/button";
import { useLayoutEffect, useRef } from "react";
import type { ComponentProps } from "react";

import { cx } from "./cx";
import styles from "./button.module.css";

type Variant = "primary" | "secondary" | "danger" | "plain";

interface ButtonProps extends Omit<ComponentProps<"button">, "disabled"> {
  variant?: Variant;
  // A held button keeps focus and its label; it just takes no clicks.
  held?: boolean;
  // A pending button shows work in progress and is held meanwhile.
  pending?: boolean;
}

function variantClass(variant: Variant): string | undefined {
  switch (variant) {
    case "primary":
      return styles.primary;
    case "danger":
      return styles.danger;
    case "plain":
      return styles.plain;
    default:
      return styles.secondary;
  }
}

export function Button({
  variant = "secondary",
  held = false,
  pending = false,
  className,
  ...props
}: ButtonProps) {
  return (
    <BaseButton
      className={cx(styles.button, variantClass(variant), className)}
      disabled={held || pending}
      focusableWhenDisabled
      aria-busy={pending || undefined}
      data-pending={pending || undefined}
      {...props}
    />
  );
}

const FOCUS_HEIR = "data-focus-heir";

// What a box wears to take the focus of a button that leaves inside it.
export const focusHeir = { tabIndex: -1, [FOCUS_HEIR]: "" } as const;

// A button its own action takes away. The focus it held goes to the
// nearest box around it that wears `focusHeir`, so the action never
// drops it; the page stays where it is scrolled.
export function LeavingButton(props: Omit<ButtonProps, "ref">) {
  const ref = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const button = ref.current;
    return () => {
      if (button !== null && button === document.activeElement) {
        button.closest<HTMLElement>(`[${FOCUS_HEIR}]`)?.focus({ preventScroll: true });
      }
    };
  }, []);
  return <Button {...props} ref={ref} />;
}
