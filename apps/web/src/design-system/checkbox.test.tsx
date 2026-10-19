// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { Checkbox } from "./checkbox";

afterEach(cleanup);

test("the box is named by its label and says whether it is ticked", () => {
  render(
    <Checkbox
      checked
      label="Select conversation"
      onCheckedChange={vi.fn<(checked: boolean, event: Event) => void>()}
    />,
  );
  const box = screen.getByRole("checkbox", { name: "Select conversation" });
  expect(box.getAttribute("aria-checked")).toBe("true");
});

test("a click reports the other state with its event, so a caller reads its keys", () => {
  const onCheckedChange = vi.fn<(checked: boolean, event: Event) => void>();
  render(<Checkbox checked={false} label="Select" onCheckedChange={onCheckedChange} />);
  fireEvent.click(screen.getByRole("checkbox"), { shiftKey: true });
  expect(onCheckedChange).toHaveBeenCalledOnce();
  const [checked, event] = onCheckedChange.mock.calls[0] ?? [];
  expect(checked).toBe(true);
  expect(event instanceof MouseEvent && event.shiftKey).toBe(true);
});

test("a box inside a composite widget stays out of the tab order and leaves the focus on a press", () => {
  render(
    <Checkbox
      checked
      label="Select"
      focusable={false}
      onCheckedChange={vi.fn<(checked: boolean, event: Event) => void>()}
    />,
  );
  const box = screen.getByRole("checkbox");
  expect(box.getAttribute("tabindex")).toBe("-1");
  expect(fireEvent.mouseDown(box)).toBe(false);
});
