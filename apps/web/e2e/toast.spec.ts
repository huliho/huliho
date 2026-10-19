// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "@playwright/test";

import { VIEWPORTS } from "./sweep";
import {
  INBOX_PATH,
  WRITABLE_READ_INBOX,
  openInbox,
  rowAt,
  subjectOf,
  threadOf,
  updates,
} from "./thread-pane";

test.describe("on a touchscreen", () => {
  test.use({ hasTouch: true, viewport: VIEWPORTS[0] });

  test("a tap on a toast's Undo takes the change back, though the touch pauses the toast", async ({
    page,
  }) => {
    const { server } = await openInbox(page, "right", INBOX_PATH, {
      mailboxes: WRITABLE_READ_INBOX,
    });
    const members = threadOf(1).map((email) => email.id);
    await rowAt(page, 1).focus();
    await page.keyboard.press("e");
    await expect.poll(() => updates(server)).toEqual([members]);
    await page.getByRole("dialog").getByRole("button", { name: "Undo" }).tap();
    await expect.poll(() => updates(server)).toEqual([members, members]);
    await expect(rowAt(page, 1)).toContainText(subjectOf(1));
  });
});
