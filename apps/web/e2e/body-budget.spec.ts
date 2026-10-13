// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "@playwright/test";

import { corpusFor, seedThread } from "./mail-corpus";
import { VIEWPORTS } from "./sweep";
import { INBOX_PATH, READ_INBOX, cards, frameIn, openInbox, rowAt } from "./thread-pane";

const INBOX_ID = "mb-inbox";
// A mail that names this many remote images, each with a text of its own.
const FLOOD_IMAGES = 20_000;
// The boxes a message draws for blocked images; the rest stay blank.
const BOXES = 100;
// The flood stands at its height about three seconds after the click on
// a desktop here and thirteen on the phone profile; a shared runner gets this long.
const FLOOD_SHOWN_MS = 20_000;

function imageFlood(): string {
  return Array.from({ length: FLOOD_IMAGES }, (_, index) => {
    const n = String(index);
    return `<img src="https://cdn.example.test/flood/${n}.png" width="40" height="20" alt="Image ${n}">`;
  }).join("");
}

test("a mail of twenty thousand remote images shows in seconds, with a box for the first hundred and the rest blank", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const corpus = corpusFor(READ_INBOX);
  seedThread(corpus, INBOX_ID, [{ seen: true, body: { html: imageFlood() } }]);
  await openInbox(page, "right", INBOX_PATH, { mail: { corpus } });
  const pressed = Date.now();
  await rowAt(page, 1).click();
  const frame = frameIn(cards(page).last());
  await expect(frame).not.toHaveAttribute("data-loading", { timeout: FLOOD_SHOWN_MS });
  console.log(`image flood: shown ${String(Date.now() - pressed)} ms after the click`);
  const message = cards(page).last().frameLocator("iframe");
  await expect(message.locator("img")).toHaveCount(FLOOD_IMAGES);
  await expect(message.locator('img[src^="data:image/svg+xml"]')).toHaveCount(BOXES);
});
