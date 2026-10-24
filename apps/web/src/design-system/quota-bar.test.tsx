// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { QuotaBar } from "./quota-bar";

const LIMIT = 1000;
const DETAIL = "Mail storage: 4.2 GB of 10 GB used";

afterEach(cleanup);

test("the meter is named by the whole sentence and reads its share", () => {
  render(
    <QuotaBar
      used={420}
      limit={LIMIT}
      locale="en"
      words="4.2 of 10 GB"
      detail={DETAIL}
      warning={null}
      variant="foot"
    />,
  );
  const meter = screen.getByRole("meter", { name: DETAIL });
  expect(meter.getAttribute("aria-valuenow")).toBe("420");
  expect(meter.getAttribute("aria-valuemax")).toBe(String(LIMIT));
  expect(meter.getAttribute("aria-valuetext")).toBe("42%");
  expect(meter.textContent).toContain("4.2 of 10 GB");
  expect(meter.querySelector<HTMLElement>("[style*='width']")?.style.width).toBe("42%");
});

test("past the warn limit the word stands before the figures and is the meter's value", () => {
  render(
    <QuotaBar
      used={910}
      limit={LIMIT}
      locale="en"
      words="9.1 of 10 GB"
      detail="Mail storage: 9.1 GB of 10 GB used"
      warning="Almost full"
      variant="foot"
    />,
  );
  const meter = screen.getByRole("meter");
  expect(meter.getAttribute("aria-valuetext")).toBe("Almost full");
  expect(meter.hasAttribute("data-warn")).toBe(true);
  expect(meter.textContent).toMatch(/^Almost full9\.1 of 10 GB/);
});

test("the rail draws the bar alone and a used figure past the limit fills it", () => {
  render(
    <QuotaBar
      used={1500}
      limit={LIMIT}
      locale="en"
      words="15 of 10 GB"
      detail={DETAIL}
      warning="Almost full"
      variant="rail"
    />,
  );
  const meter = screen.getByRole("meter", { name: DETAIL });
  expect(meter.textContent).not.toContain("of 10 GB");
  expect(meter.getAttribute("aria-valuenow")).toBe(String(LIMIT));
  expect(meter.querySelector<HTMLElement>("[style*='width']")?.style.width).toBe("100%");
});
