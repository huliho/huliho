// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Button as BaseButton } from "@base-ui/react/button";
import type { ComponentProps } from "react";

import { cx } from "./cx";
import styles from "./button.module.css";

type Variant = "primary" | "secondary" | "danger";

interface ButtonProps extends Omit<ComponentProps<"button">, "disabled"> {
  variant?: Variant;
  // A held button keeps focus and its label; it just takes no clicks.
  held?: boolean;
  // A pending button shows work in progress and is held meanwhile.
  pending?: boolean;
}

function variantClass(variant: Variant): string | undefined {
  if (variant === "primary") {
    return styles.primary;
  }
  return variant === "danger" ? styles.danger : styles.secondary;
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
