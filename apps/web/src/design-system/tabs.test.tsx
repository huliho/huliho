// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";

import { Tabs } from "./tabs";

type Value = "one" | "two" | "three";

const TABS = [
  { value: "one", label: "One" },
  { value: "two", label: "Two" },
  { value: "three", label: "Three" },
] as const;

function Harness() {
  const [value, setValue] = useState<Value>("one");
  return (
    <Tabs
      value={value}
      onValueChange={setValue}
      tabs={TABS}
      panelClassName="sheet"
      panels={{
        one: <p>first panel</p>,
        two: <p>second panel</p>,
        three: <p>third panel</p>,
      }}
    />
  );
}

afterEach(cleanup);

test("a tab list over one panel: the active tab's panel alone is in the tree and named by its tab", () => {
  render(<Harness />);
  const tabs = within(screen.getByRole("tablist")).getAllByRole("tab");
  expect(tabs.map((tab) => tab.textContent)).toEqual(["One", "Two", "Three"]);
  expect(tabs[0]?.getAttribute("aria-selected")).toBe("true");
  const panel = screen.getByRole("tabpanel");
  expect(panel.textContent).toBe("first panel");
  expect(panel.getAttribute("aria-labelledby")).toBe(tabs[0]?.id);
  expect(panel.classList.contains("sheet")).toBe(true);
  expect(screen.queryByText("second panel")).toBeNull();
  fireEvent.click(tabs[1] ?? panel);
  expect(screen.getByRole("tabpanel").textContent).toBe("second panel");
  expect(screen.queryByText("first panel")).toBeNull();
});

test("the arrow keys move the focus along the tabs and show the one it lands on; the others leave the tab order", async () => {
  render(<Harness />);
  const [first, second] = screen.getAllByRole("tab");
  if (first === undefined || second === undefined) {
    throw new Error("the row has fewer than two tabs");
  }
  first.focus();
  fireEvent.keyDown(first, { key: "ArrowRight" });
  // The focus moves once the row has let go of the key.
  await waitFor(() => {
    expect(document.activeElement).toBe(second);
  });
  expect(second.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tabpanel").textContent).toBe("second panel");
  expect(first.getAttribute("tabindex")).toBe("-1");
  expect(screen.getByRole("tabpanel").getAttribute("tabindex")).toBe("0");
});
