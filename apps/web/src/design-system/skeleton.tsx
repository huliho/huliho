// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cx } from "./cx";
import styles from "./skeleton.module.css";

interface SkeletonProps {
  className?: string | undefined;
}

// A still block in the shape of the content it stands in for.
export function Skeleton({ className }: SkeletonProps) {
  return <span aria-hidden="true" className={cx(styles.skeleton, className)} />;
}
