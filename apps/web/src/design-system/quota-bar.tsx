// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Meter } from "@base-ui/react/meter";
import type { ComponentPropsWithRef } from "react";

import { cx } from "./cx";
import styles from "./quota-bar.module.css";

interface QuotaBarProps extends Omit<ComponentPropsWithRef<"div">, "children"> {
  // How much is used of how much, in the quota's own unit.
  used: number;
  limit: number;
  locale: string;
  // The words over the bar: "4.2 of 10 GB".
  words: string;
  // The whole sentence, which names the meter: "Mail storage: 4.2 GB of 10 GB used".
  detail: string;
  // The word that warns, once the quota reaches its warn limit.
  warning: string | null;
  // The sidebar's foot with the words over the bar, or the bar alone in the rail.
  variant: "foot" | "rail";
}

// A quota as a meter: the words over a 2 px bar, the bar in the warn
// tone with its word once the quota reaches its warn limit. The meter is
// named by the whole sentence and reads the warning as its value.
export function QuotaBar(props: QuotaBarProps) {
  const { used, limit, locale, words, detail, warning, variant, className, ...rest } = props;
  return (
    <Meter.Root
      {...rest}
      value={used}
      max={limit}
      locale={locale}
      aria-label={detail}
      getAriaValueText={(percent) => warning ?? percent}
      data-warn={warning === null ? undefined : ""}
      className={cx(styles.meter, variant === "rail" ? styles.rail : styles.foot, className)}
    >
      {variant === "foot" && (
        <span className={styles.words}>
          {warning !== null && <span className={styles.warning}>{warning}</span>}
          <span className={styles.figures}>{words}</span>
        </span>
      )}
      <Meter.Track className={styles.track}>
        <Meter.Indicator className={styles.fill} />
      </Meter.Track>
    </Meter.Root>
  );
}
