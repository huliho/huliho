// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { PreferenceChange } from "@huliho/core";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import type { Locale } from "../../paraglide/runtime.js";
import { AppearanceForm } from "./appearance-form";

interface Rendered {
  onChange: Mock<(change: PreferenceChange) => void>;
  onSwitchLocale: Mock<(locale: Locale) => void>;
}

function renderForm(preferences: Parameters<typeof AppearanceForm>[0]["preferences"]): Rendered {
  const rendered: Rendered = {
    onChange: vi.fn<(change: PreferenceChange) => void>(),
    onSwitchLocale: vi.fn<(locale: Locale) => void>(),
  };
  render(<AppearanceForm locale="en" preferences={preferences} {...rendered} />);
  return rendered;
}

function checkedIn(group: string): string {
  const radios = within(screen.getByRole("radiogroup", { name: group })).getAllByRole("radio");
  return radios.find((radio) => radio.getAttribute("aria-checked") === "true")?.textContent ?? "";
}

afterEach(cleanup);

test("four named groups show the choices, with the defaults for keys never chosen", () => {
  renderForm({ theme: "dark", readingPane: "off" });
  expect(checkedIn("Theme")).toBe("Dark");
  expect(checkedIn("Density")).toBe("Comfortable");
  expect(checkedIn("Reading pane")).toBe("Off");
  expect(checkedIn("Language")).toBe("English");
});

test("a hint describes its group and a group without one carries no description", () => {
  renderForm({});
  expect(
    screen.getByRole("radiogroup", {
      name: "Density",
      description: "Compact fits more on the screen. A touchscreen keeps its own spacing.",
    }),
  ).toBeDefined();
  expect(
    screen.getByRole("radiogroup", {
      name: "Reading pane",
      description: /^Where a conversation opens/,
    }),
  ).toBeDefined();
  expect(screen.getByRole("radiogroup", { name: "Theme" }).getAttribute("aria-describedby")).toBe(
    null,
  );
});

test("a pick names its key and word; the language group switches instead", () => {
  const rendered = renderForm({});
  fireEvent.click(screen.getByRole("radio", { name: "Compact" }));
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "density", value: "compact" });
  fireEvent.click(screen.getByRole("radio", { name: "Bottom" }));
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "readingPane", value: "bottom" });
  fireEvent.click(screen.getByRole("radio", { name: "Nederlands" }));
  expect(rendered.onSwitchLocale).toHaveBeenLastCalledWith("nl");
  expect(rendered.onChange).toHaveBeenCalledTimes(2);
});

test("a development build lists the pseudo locale in its own spelling", () => {
  renderForm({});
  const languages = within(screen.getByRole("radiogroup", { name: "Language" }));
  expect(languages.getAllByRole("radio").map((radio) => radio.textContent)).toEqual([
    "English",
    "Nederlands",
    "English (Pseudo-Accents)",
  ]);
});
