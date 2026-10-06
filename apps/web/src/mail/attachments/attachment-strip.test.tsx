// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { ATTACHMENT_PARTS, attachmentsDetail } from "../body-fixtures";
import { DOWNLOAD_PREFIX } from "../body/frame-rig";
import { AttachmentStrip } from "./attachment-strip";
import { attachmentsOf } from "./parts";

const NONE: ReadonlySet<string> = new Set();
const ATTACHMENTS = attachmentsOf(attachmentsDetail("e-3"), NONE, "en");
const WARNING = "This file can run programs on your computer.";

afterEach(cleanup);

function renderStrip(): void {
  render(<AttachmentStrip locale="en" attachments={ATTACHMENTS} />);
}

function list(): HTMLElement {
  return screen.getByRole("list", { name: `${String(ATTACHMENT_PARTS.length)} attachments` });
}

test("the chips come first and the preview after them, each a link that downloads or a button", () => {
  renderStrip();
  const items = within(list()).getAllByRole("listitem");
  expect(items).toHaveLength(ATTACHMENT_PARTS.length);
  // The name is read out whole, however it is drawn.
  const pdf = within(items[0] ?? document.body).getByRole("link", {
    name: "Offerte_badkamer_renovatie_v3_definitief.pdf 48 kB",
  });
  expect(pdf.getAttribute("href")).toBe(
    `${DOWNLOAD_PREFIX}b-offerte/Offerte_badkamer_renovatie_v3_definitief.pdf?type=application%2Foctet-stream`,
  );
  expect(pdf.hasAttribute("download")).toBe(true);
  expect(screen.getByRole("link", { name: "Unnamed attachment 1 kB" })).toBeDefined();
  expect(screen.getByRole("link", { name: "Attached message 12 kB" }).getAttribute("href")).toBe(
    `${DOWNLOAD_PREFIX}b-forwarded/message.eml?type=application%2Foctet-stream`,
  );
  // The file that can run a program is a button, so nothing downloads on its own.
  expect(screen.getByRole("button", { name: "factuur_viewer.html 36 kB" })).toBeDefined();
  const preview = within(items.at(-1) ?? document.body).getByRole("button", {
    name: "tegelwerk_voorbeeld.jpg 2.4 MB",
  });
  expect(preview.querySelector("img")?.getAttribute("src")).toBe(
    `${DOWNLOAD_PREFIX}b-photo/tegelwerk_voorbeeld.jpg?type=image%2Fjpeg`,
  );
});

test("an image under a name that asks first is a chip that asks, never a preview", () => {
  renderStrip();
  const chip = screen.getByRole("button", { name: "kaart.html 20 kB" });
  expect(chip.querySelector("img")).toBeNull();
  fireEvent.click(chip);
  expect(screen.getByRole("dialog", { name: WARNING })).toBeDefined();
});

test("an image the server did not serve as that image becomes a chip", () => {
  renderStrip();
  const image = screen.getByRole("button", { name: "tegelwerk_voorbeeld.jpg 2.4 MB" });
  fireEvent.error(image.querySelector("img") ?? image);
  expect(screen.queryByRole("button", { name: "tegelwerk_voorbeeld.jpg 2.4 MB" })).toBeNull();
  const chip = screen.getByRole("link", { name: "tegelwerk_voorbeeld.jpg 2.4 MB" });
  expect(chip.getAttribute("href")).toBe(
    `${DOWNLOAD_PREFIX}b-photo/tegelwerk_voorbeeld.jpg?type=application%2Foctet-stream`,
  );
});

test("a file that can run a program asks first, with Cancel in focus; Cancel hands the focus back to the chip", async () => {
  renderStrip();
  const chip = screen.getByRole("button", { name: "factuur_viewer.html 36 kB" });
  chip.focus();
  fireEvent.click(chip);
  const dialog = await screen.findByRole("dialog", { name: WARNING });
  const description = document.getElementById(dialog.getAttribute("aria-describedby") ?? "");
  expect(description?.textContent).toBe("factuur_viewer.html · 36 kB");
  // The name is mail content and reads in its own direction.
  expect(description?.querySelector("bdi")?.textContent).toBe("factuur_viewer.html");
  const cancel = within(dialog).getByRole("button", { name: "Cancel" });
  await waitFor(() => {
    expect(document.activeElement).toBe(cancel);
  });
  const anyway = within(dialog).getByRole("link", { name: "Download anyway" });
  expect(anyway.getAttribute("href")).toBe(
    `${DOWNLOAD_PREFIX}b-viewer/factuur_viewer.html?type=application%2Foctet-stream`,
  );
  expect(anyway.hasAttribute("download")).toBe(true);
  fireEvent.click(cancel);
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  await waitFor(() => {
    expect(document.activeElement).toBe(chip);
  });
});

test("Download anyway closes the question and hands the focus back as well", async () => {
  renderStrip();
  const chip = screen.getByRole("button", { name: "factuur_viewer.html 36 kB" });
  chip.focus();
  fireEvent.click(chip);
  const anyway = await screen.findByRole("link", { name: "Download anyway" });
  fireEvent.click(anyway);
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  await waitFor(() => {
    expect(document.activeElement).toBe(chip);
  });
});

test("a preview opens the image at full size, named and with Download; Escape closes it and the focus returns", async () => {
  renderStrip();
  const preview = screen.getByRole("button", { name: "tegelwerk_voorbeeld.jpg 2.4 MB" });
  preview.focus();
  fireEvent.click(preview);
  const dialog = await screen.findByRole("dialog", { name: "tegelwerk_voorbeeld.jpg" });
  expect(within(dialog).getByRole("heading").querySelector("bdi")?.textContent).toBe(
    "tegelwerk_voorbeeld.jpg",
  );
  expect(within(dialog).getByText("2.4 MB")).toBeDefined();
  expect(within(dialog).getByRole("link", { name: "Download" }).getAttribute("href")).toBe(
    `${DOWNLOAD_PREFIX}b-photo/tegelwerk_voorbeeld.jpg?type=application%2Foctet-stream`,
  );
  expect(
    within(dialog).getByRole("img", { name: "tegelwerk_voorbeeld.jpg" }).getAttribute("src"),
  ).toBe(`${DOWNLOAD_PREFIX}b-photo/tegelwerk_voorbeeld.jpg?type=image%2Fjpeg`);
  fireEvent.keyDown(document.activeElement ?? dialog, { key: "Escape" });
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  await waitFor(() => {
    expect(document.activeElement).toBe(preview);
  });
  // The close button does the same.
  fireEvent.click(preview);
  fireEvent.click(await screen.findByRole("button", { name: "Close" }));
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
