// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { Button } from "../../design-system/button";
import { Dialog, DialogActions } from "../../design-system/dialog";
import { Field } from "../../design-system/field";
import type { Locale } from "../../paraglide/runtime.js";
import type { FolderRefusal, NameAsk } from "./folder-asks";
import { ConfirmDialog, NameDialog } from "./folder-dialogs";
import { managedOf } from "./writable-rig";

// A name of six octets against a limit of five.
const LIMIT_BYTES = 5;

function parts() {
  return {
    Dialog,
    DialogActions,
    Field,
    Button,
    close: vi.fn<() => void>(),
    settle: vi.fn<() => void>(),
  };
}

function nameAsk(extra: Partial<NameAsk> = {}): NameAsk {
  return {
    kind: "create",
    name: "",
    place: null,
    taken: ["Facturen"],
    limit: 255,
    write: vi.fn<(name: string) => Promise<FolderRefusal | null>>(() => Promise.resolve(null)),
    ...extra,
  };
}

function drawName(ask: NameAsk, locale: Locale = "en") {
  const given = parts();
  render(<NameDialog locale={locale} ask={ask} open parts={given} />);
  const dialog = screen.getByRole("dialog");
  const field = within(dialog).getByRole("textbox", {
    name: locale === "en" ? "Folder name" : "Mapnaam",
  });
  return { given, dialog, field };
}

function submit(field: HTMLElement, value: string): void {
  fireEvent.change(field, { target: { value } });
  const form = field.closest("form");
  if (form === null) {
    throw new Error("the field stands in no form");
  }
  fireEvent.submit(form);
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("a new folder's dialog names its place; an empty name never goes out", () => {
  const ask = nameAsk();
  const { dialog, field } = drawName(ask);
  expect(screen.getByRole("dialog", { name: "New folder" })).toBe(dialog);
  expect(dialog.textContent).toContain("At the top level");
  submit(field, "   ");
  expect(within(dialog).getByRole("alert").textContent).toContain("Give the folder a name.");
  expect(ask.write).not.toHaveBeenCalled();
});

test("a name past the account's octets or the name beside it is refused on the field and kept", () => {
  const ask = nameAsk({ limit: LIMIT_BYTES, place: "Verbouwing" });
  const { dialog, field } = drawName(ask);
  expect(dialog.textContent).toContain("In Verbouwing");
  submit(field, "Kasten");
  expect(within(dialog).getByRole("alert").textContent).toContain("The name is too long.");
  expect(field).toHaveProperty("value", "Kasten");
  fireEvent.change(field, { target: { value: "Kast" } });
  expect(within(dialog).queryByRole("alert")).toBeNull();
  cleanup();
  const again = drawName(nameAsk());
  submit(again.field, "Facturen");
  expect(within(again.dialog).getByRole("alert").textContent).toContain(
    "A folder with that name is already there.",
  );
  expect(ask.write).not.toHaveBeenCalled();
});

test("a name goes out trimmed and in NFC; the primary waits and Escape holds until the answer", async () => {
  const answer = Promise.withResolvers<FolderRefusal | null>();
  const ask = nameAsk({ write: vi.fn<NameAsk["write"]>(() => answer.promise) });
  const { given, dialog, field } = drawName(ask);
  submit(field, " Café ");
  expect(ask.write).toHaveBeenCalledWith("Café");
  const create = within(dialog).getByRole("button", { name: "Create" });
  expect(create.getAttribute("aria-busy")).toBe("true");
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(given.close).not.toHaveBeenCalled();
  await act(async () => {
    answer.resolve(null);
    await answer.promise;
  });
  expect(given.close).toHaveBeenCalledOnce();
});

test("the server's refusal and the missing connection read in the failure line; the text stays", async () => {
  const ask = nameAsk({ write: vi.fn<NameAsk["write"]>(() => Promise.resolve("name")) });
  const { dialog, field } = drawName(ask);
  await act(async () => {
    submit(field, "a/b");
    await Promise.resolve();
  });
  expect(within(dialog).getByRole("alert").textContent).toBe(
    "The server doesn’t accept that name.",
  );
  expect(field).toHaveProperty("value", "a/b");
  cleanup();
  const failing = drawName(
    nameAsk({ write: vi.fn<NameAsk["write"]>(() => Promise.resolve("server")) }),
  );
  await act(async () => {
    submit(failing.field, "Projecten");
    await Promise.resolve();
  });
  expect(within(failing.dialog).getByRole("alert").textContent).toBe(
    "The server refused. Try again.",
  );
  cleanup();
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  const offline = nameAsk();
  const away = drawName(offline);
  submit(away.field, "Projecten");
  expect(within(away.dialog).getByRole("alert").textContent).toBe("Changes need a connection.");
  expect(offline.write).not.toHaveBeenCalled();
});

test("a server refusal puts the focus in the field, from the primary button too", async () => {
  const ask = nameAsk({ write: vi.fn<NameAsk["write"]>(() => Promise.resolve("name")) });
  const { dialog, field } = drawName(ask);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(field);
  });
  within(dialog).getByRole("button", { name: "Create" }).focus();
  await act(async () => {
    submit(field, "a/b");
    await Promise.resolve();
  });
  expect(within(dialog).getByRole("alert").textContent).toBe(
    "The server doesn’t accept that name.",
  );
  expect(document.activeElement).toBe(field);
});

test("a rename holds the name selected and is worded in the reader's language", () => {
  const { dialog, field } = drawName(
    nameAsk({ kind: "rename", name: "Offertes", place: "Verbouwing" }),
    "nl",
  );
  expect(screen.getByRole("dialog", { name: "Naam van map wijzigen" })).toBe(dialog);
  expect(dialog.textContent).toContain("In Verbouwing");
  fireEvent.focus(field);
  expect(field).toHaveProperty("selectionStart", 0);
  expect(field).toHaveProperty("selectionEnd", "Offertes".length);
  expect(within(dialog).getByRole("button", { name: "Naam wijzigen" })).toBeDefined();
});

test("the delete confirm counts the folder's mail, holds Cancel first and runs its action once the dialog has closed", async () => {
  const confirm = vi.fn<() => void>();
  const given = parts();
  const ask = { kind: "delete" as const, mailbox: managedOf("mb-offertes"), confirm };
  const { rerender } = render(<ConfirmDialog locale="en" ask={ask} open parts={given} />);
  const dialog = screen.getByRole("dialog", { name: "Delete “Offertes”?" });
  expect(dialog.textContent).toContain(
    "Its 5 messages will be deleted forever unless they are also in another folder. This can’t be undone.",
  );
  const cancel = within(dialog).getByRole("button", { name: "Cancel" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(cancel);
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Delete folder" }));
  expect(given.close).toHaveBeenCalledOnce();
  expect(confirm).not.toHaveBeenCalled();
  rerender(<ConfirmDialog locale="en" ask={ask} open={false} parts={given} />);
  await vi.waitFor(() => {
    expect(confirm).toHaveBeenCalledOnce();
  });
  expect(given.settle).toHaveBeenCalledOnce();
});
