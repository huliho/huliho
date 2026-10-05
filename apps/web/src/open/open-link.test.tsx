// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { openHref } from "./fragment";
import { linkKey } from "./link-key";
import { OpenLinkPage } from "./open-link";
import type { Tab } from "./open-link";

function opened(target: string, text = "", key = linkKey()) {
  const tab = {
    hash: (openHref({ target, text, key }) ?? "").slice("/open".length),
    hostname: "mail.example.test",
    replace: vi.fn<Tab["replace"]>(),
    hand: vi.fn<Tab["hand"]>(),
    close: vi.fn<Tab["close"]>(),
  };
  render(<OpenLinkPage tab={tab} />);
  return tab;
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

test("a plain link leaves for its target at once and says where it goes", () => {
  const tab = opened("https://shop.example/sale?x=1", "See the sale");
  expect(tab.replace).toHaveBeenCalledExactlyOnceWith("https://shop.example/sale?x=1");
  expect(screen.getByText("Opening shop.example…")).toBeDefined();
  expect(screen.queryByRole("button")).toBeNull();
  expect(tab.close).not.toHaveBeenCalled();
});

test("a mail address goes to the mail program and the tab stays for the reader to close", () => {
  const tab = opened("mailto:sanne@example.test?subject=hi");
  expect(tab.hand).toHaveBeenCalledExactlyOnceWith("mailto:sanne@example.test?subject=hi");
  expect(tab.close).not.toHaveBeenCalled();
  expect(tab.replace).not.toHaveBeenCalled();
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Opening your mail program…");
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(tab.close).toHaveBeenCalledOnce();
});

test("a link whose text names another host asks, with Cancel in focus and the target in full", () => {
  const target = "https://mybank-secure.example.net/login/verify?session=8f3a";
  const tab = opened(target, "mybank.example");
  expect(tab.replace).not.toHaveBeenCalled();
  const sentence = screen.getByRole("heading", { level: 2 });
  expect(sentence.textContent).toBe(
    "This link says mybank.example but opens mybank-secure.example.net.",
  );
  const cancel = screen.getByRole("button", { name: "Cancel" });
  expect(document.activeElement).toBe(cancel);
  expect(cancel.getAttribute("aria-describedby")).toBe(sentence.id);
  expect(screen.getByText("mybank-secure.example.net").parentElement?.textContent).toBe(target);
  fireEvent.click(cancel);
  expect(tab.close).toHaveBeenCalledOnce();
  expect(tab.replace).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Open anyway" }));
  expect(tab.replace).toHaveBeenCalledExactlyOnceWith(target);
});

test.each([
  [
    "https://xn--mybnk-fsa.example/inloggen",
    "This link opens an internationalized domain, xn--mybnk-fsa.example.",
  ],
  [
    "https://mail.example.test/settings/accounts",
    "This link opens your own mail app at /settings/accounts.",
  ],
])("a risky target asks with its own sentence: %s", (target, sentence) => {
  const tab = opened(target, "Sign in");
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe(sentence);
  expect(tab.replace).not.toHaveBeenCalled();
});

test("a link another device or page made asks before it opens", () => {
  const tab = opened("https://evil.example/landing", "", "not-this-device");
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe(
    "This link opens evil.example.",
  );
  expect(tab.replace).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
});

test("a link that cannot be opened says so and offers the way out", () => {
  const tab = opened("javascript:top.__x=1");
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("This link can’t be opened.");
  expect(tab.replace).not.toHaveBeenCalled();
  expect(tab.hand).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(tab.close).toHaveBeenCalledOnce();
});

test("the page carries the instance's mark as its heading", () => {
  opened("https://evil.example/", "", "not-this-device");
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Huliho");
});
