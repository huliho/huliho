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

function group(name: string): HTMLElement {
  return screen.getByRole("radiogroup", { name });
}

function checkedIn(name: string): string {
  const radios = within(group(name)).getAllByRole("radio");
  return radios.find((radio) => radio.getAttribute("aria-checked") === "true")?.textContent ?? "";
}

// The word of the group, since two cards offer a word of the same spelling.
function pick(name: string, word: string): void {
  fireEvent.click(within(group(name)).getByRole("radio", { name: word }));
}

afterEach(cleanup);

test("seven named groups show the choices in order, with the defaults for keys never chosen", () => {
  renderForm({ theme: "dark", readingPane: "off", lineHeight: "loose" });
  expect(
    screen.getAllByRole("radiogroup").map((found) => found.getAttribute("aria-labelledby")),
  ).toEqual(screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.id));
  expect(
    screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent),
  ).toEqual([
    "Theme",
    "Density",
    "Font size",
    "Line height",
    "Dark mode for messages",
    "Reading pane",
    "Language",
  ]);
  expect(checkedIn("Theme")).toBe("Dark");
  expect(checkedIn("Density")).toBe("Comfortable");
  expect(checkedIn("Font size")).toBe("Default");
  expect(checkedIn("Line height")).toBe("Loose");
  expect(checkedIn("Dark mode for messages")).toBe("Adapt light messages");
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
      name: "Font size",
      description: "Applies to the app and to messages.",
    }),
  ).toBeDefined();
  expect(
    screen.getByRole("radiogroup", {
      name: "Reading pane",
      description: /^Where a conversation opens/,
    }),
  ).toBeDefined();
  expect(
    screen.getByRole("radiogroup", {
      name: "Dark mode for messages",
      description: /^A light message takes the dark theme’s colors/,
    }),
  ).toBeDefined();
  for (const name of ["Theme", "Line height", "Language"]) {
    expect(group(name).getAttribute("aria-describedby")).toBe(null);
  }
});

test("a pick names its key and word; the language group switches instead", () => {
  const rendered = renderForm({});
  pick("Density", "Compact");
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "density", value: "compact" });
  pick("Font size", "Larger");
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "fontSize", value: "larger" });
  pick("Line height", "Relaxed");
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "lineHeight", value: "relaxed" });
  pick("Reading pane", "Bottom");
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "readingPane", value: "bottom" });
  pick("Dark mode for messages", "Show as sent");
  expect(rendered.onChange).toHaveBeenLastCalledWith({ key: "darkMail", value: "original" });
  pick("Language", "Nederlands");
  expect(rendered.onSwitchLocale).toHaveBeenLastCalledWith("nl");
  expect(rendered.onChange).toHaveBeenCalledTimes(5);
});

test("a development build lists the pseudo locale in its own spelling", () => {
  renderForm({});
  const languages = within(group("Language"));
  expect(languages.getAllByRole("radio").map((radio) => radio.textContent)).toEqual([
    "English",
    "Nederlands",
    "English (Pseudo-Accents)",
  ]);
});
