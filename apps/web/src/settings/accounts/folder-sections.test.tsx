// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { closedFolders, hiddenFolders, roleRows } from "./folder-choices";
import type { RoleName } from "./folder-choices";
import { FOLDERS, closedBox, fixedBox, hiddenBox } from "./folder-fixtures";
import {
  ClosedSection,
  HiddenSection,
  RolesSection,
  closedControl,
  roleControl,
  showControl,
} from "./folder-sections";
import type { ClosedChoice } from "./folder-sections";
import type { PendingWrite } from "./use-folder-writes";

interface RolesFacts {
  mailboxes?: readonly Mailbox[];
  online?: boolean;
  pending?: PendingWrite | null;
}

function drawRoles({ mailboxes = FOLDERS, online = true, pending = null }: RolesFacts = {}) {
  const onRole = vi.fn<(role: RoleName, takerId: string | null) => void>();
  render(
    <RolesSection
      locale="en"
      name="Sanne"
      rows={roleRows(mailboxes, "en")}
      online={online}
      pending={pending}
      onRole={onRole}
    />,
  );
  return onRole;
}

function select(name: string): HTMLSelectElement {
  const found = screen.getByRole("combobox", { name });
  if (!(found instanceof HTMLSelectElement)) {
    throw new TypeError(`${name} is no select`);
  }
  return found;
}

afterEach(cleanup);

test("the heading is the account's name; each of the six roles is a select with the holder chosen", () => {
  drawRoles();
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Sanne");
  expect(screen.getAllByRole("combobox").map((row) => row.getAttribute("aria-label"))).toHaveLength(
    6,
  );
  expect(select("Inbox").value).toBe("mb-inbox");
  expect(select("Archive").value).toBe("mb-archive");
  expect(select("Junk").value).toBe("");
  expect(select("Junk").selectedOptions[0]?.textContent).toBe("None");
});

test("a change names the taker; None names no mailbox", () => {
  const onRole = drawRoles();
  fireEvent.change(select("Archive"), { target: { value: "mb-facturen" } });
  expect(onRole).toHaveBeenLastCalledWith("archive", "mb-facturen");
  fireEvent.change(select("Sent"), { target: { value: "" } });
  expect(onRole).toHaveBeenLastCalledWith("sent", null);
});

test("the row of a mailbox the server fixes takes no change and offers no None", () => {
  drawRoles();
  expect(select("Inbox").disabled).toBe(true);
  expect(Array.from(select("Inbox").options).map(({ value }) => value)).not.toContain("");
  expect(select("Archive").disabled).toBe(false);
});

test("the inbox alone needs no sentence; another fixed row says the server sets it", () => {
  drawRoles();
  expect(screen.queryByText("The server sets the folders you can’t change here.")).toBeNull();
  cleanup();
  drawRoles({ mailboxes: [...FOLDERS, fixedBox("mb-spam", "Spam", "junk")] });
  expect(screen.getByText("The server sets the folders you can’t change here.")).toBeDefined();
});

test("offline every select is disabled and one sentence says why", () => {
  drawRoles({ online: false });
  for (const row of screen.getAllByRole("combobox")) {
    expect(row).toHaveProperty("disabled", true);
  }
  expect(screen.getByText("Changes need a connection.")).toBeDefined();
});

test("a pick on its way stands on its row and marks that row busy alone", () => {
  drawRoles({ pending: { control: roleControl("archive"), value: "mb-facturen" } });
  expect(select("Archive").value).toBe("mb-facturen");
  expect(select("Archive").getAttribute("aria-busy")).toBe("true");
  expect(select("Sent").getAttribute("aria-busy")).toBeNull();
});

function drawHidden(
  mailboxes: readonly Mailbox[],
  online = true,
  pending: PendingWrite | null = null,
) {
  const onShow = vi.fn<(id: string) => void>();
  const rendered = render(
    <HiddenSection
      locale="en"
      folders={hiddenFolders(mailboxes, "en")}
      online={online}
      pending={pending}
      onShow={onShow}
    />,
  );
  return { onShow, rendered };
}

const WITH_HIDDEN = [
  ...FOLDERS,
  hiddenBox("mb-kelder", "Kelder"),
  hiddenBox("mb-oud", "Oud", "mb-kelder"),
];

test("each hidden folder has its own Show, named by its path; nothing hidden says so", () => {
  const { onShow } = drawHidden(WITH_HIDDEN);
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Show Kelder / Oud" }));
  expect(onShow).toHaveBeenCalledExactlyOnceWith("mb-oud");
  cleanup();
  drawHidden(FOLDERS);
  expect(screen.getByText("No hidden folders")).toBeDefined();
  expect(screen.queryByRole("list")).toBeNull();
});

test("a Show on its way is held and says so; offline every Show is held", () => {
  const pending = { control: showControl("mb-kelder"), value: "mb-kelder" };
  const { onShow } = drawHidden(WITH_HIDDEN, true, pending);
  const button = screen.getByRole("button", { name: "Show Kelder" });
  expect(button.getAttribute("aria-busy")).toBe("true");
  fireEvent.click(button);
  expect(onShow).not.toHaveBeenCalled();
  cleanup();
  const offline = drawHidden(WITH_HIDDEN, false);
  fireEvent.click(screen.getByRole("button", { name: "Show Kelder" }));
  expect(offline.onShow).not.toHaveBeenCalled();
});

test("a Show that takes its row away hands the focus to the box around the list", () => {
  const { rendered } = drawHidden(WITH_HIDDEN);
  const button = screen.getByRole("button", { name: "Show Kelder" });
  button.focus();
  rendered.rerender(
    <HiddenSection
      locale="en"
      folders={hiddenFolders(FOLDERS, "en")}
      online
      pending={null}
      onShow={vi.fn<(id: string) => void>()}
    />,
  );
  expect(document.activeElement?.getAttribute("data-focus-heir")).toBe("");
});

function drawClosed(
  mailboxes: readonly Mailbox[],
  online = true,
  pending: PendingWrite | null = null,
) {
  const onClosed = vi.fn<(id: string, choice: ClosedChoice) => void>();
  render(
    <ClosedSection
      locale="en"
      folders={closedFolders(mailboxes, "en")}
      online={online}
      pending={pending}
      onClosed={onClosed}
    />,
  );
  return onClosed;
}

const WITH_CLOSED = [...FOLDERS, closedBox("mb-dicht", "Dicht", { subscribed: true })];

test("a closed mailbox offers the link and Hide, the link chosen while the tree draws it", () => {
  const onClosed = drawClosed(WITH_CLOSED);
  expect(screen.getByRole("radiogroup", { name: "Dicht" })).toBeDefined();
  const link = screen.getByRole("radio", { name: "Show as a link" });
  expect(link.getAttribute("aria-checked")).toBe("true");
  fireEvent.click(screen.getByRole("radio", { name: "Hide" }));
  expect(onClosed).toHaveBeenCalledExactlyOnceWith("mb-dicht", "hide");
});

test("a hidden closed mailbox has Hide chosen; a choice on its way stands", () => {
  drawClosed([...FOLDERS, closedBox("mb-dicht", "Dicht")]);
  expect(screen.getByRole("radio", { name: "Hide" }).getAttribute("aria-checked")).toBe("true");
  cleanup();
  drawClosed(WITH_CLOSED, true, { control: closedControl("mb-dicht"), value: "hide" });
  expect(screen.getByRole("radio", { name: "Hide" }).getAttribute("aria-checked")).toBe("true");
});

test("offline the choice takes no press", () => {
  const onClosed = drawClosed(WITH_CLOSED, false);
  fireEvent.click(screen.getByRole("radio", { name: "Hide" }));
  expect(onClosed).not.toHaveBeenCalled();
});
