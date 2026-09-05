// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { IssuedSecret } from "./issued-secret";

const ONE_TIME = "k7fq-2mzp-x4rt";

type WriteText = (text: string) => Promise<void>;

// jsdom has no clipboard; the stub stands in for the one the browser offers.
function stubClipboard(writeText: WriteText): void {
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
}

afterEach(cleanup);

test("copy puts the secret on the clipboard and says so", async () => {
  const writeText = vi.fn<WriteText>().mockResolvedValue(undefined);
  stubClipboard(writeText);
  render(<IssuedSecret locale="en" secret={ONE_TIME} onDone={() => undefined} />);
  expect(screen.getByRole("button", { name: "Copy" })).toBe(document.activeElement);
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  expect(await screen.findByText("Copied")).toBeDefined();
  expect(writeText).toHaveBeenCalledExactlyOnceWith(ONE_TIME);
});

test("a refused clipboard says to copy by hand and keeps the secret readable", async () => {
  stubClipboard(() => Promise.reject(new Error("denied")));
  render(<IssuedSecret locale="en" secret={ONE_TIME} onDone={() => undefined} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  expect(await screen.findByText(/copy by hand/)).toBeDefined();
  expect(screen.getByLabelText("One-time password").textContent).toBe(ONE_TIME);
});
