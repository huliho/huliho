// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { RadioGroup } from "./radio-group";

const OPTIONS = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

afterEach(cleanup);

test("the group is named by its label and each segment by its word", () => {
  render(
    <>
      <h2 id="theme">Theme</h2>
      <RadioGroup
        labelledBy="theme"
        options={OPTIONS}
        value="light"
        onChange={vi.fn<(value: string) => void>()}
      />
    </>,
  );
  expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeDefined();
  expect(screen.getAllByRole("radio")).toHaveLength(3);
  expect(screen.getByRole("radio", { name: "Light" }).getAttribute("aria-checked")).toBe("true");
  expect(screen.getByRole("radio", { name: "Dark" }).getAttribute("aria-checked")).toBe("false");
});

test("a sentence named as the description reaches the group", () => {
  render(
    <>
      <h2 id="theme">Theme</h2>
      <RadioGroup
        labelledBy="theme"
        describedBy="theme-hint"
        options={OPTIONS}
        value="light"
        onChange={vi.fn<(value: string) => void>()}
      />
      <p id="theme-hint">System follows the device.</p>
    </>,
  );
  expect(
    screen.getByRole("radiogroup", { name: "Theme", description: "System follows the device." }),
  ).toBeDefined();
});

// Arrow keys move the choice as well; the browser suite proves that on the page.
test("a click picks a segment and reports its word alone", () => {
  const onChange = vi.fn<(value: "system" | "light" | "dark") => void>();
  render(
    <>
      <h2 id="theme">Theme</h2>
      <RadioGroup labelledBy="theme" options={OPTIONS} value="light" onChange={onChange} />
    </>,
  );
  fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
  expect(onChange).toHaveBeenCalledExactlyOnceWith("dark");
});
