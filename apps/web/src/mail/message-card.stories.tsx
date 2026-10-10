// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail, Preferences } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef } from "react";
import type { JSX } from "react";

import {
  ATTACHMENT_PARTS,
  NEWSLETTER_HTML,
  PASSED,
  PHOTO_PART,
  REPLY_TEXT,
  attachmentsDetail,
  htmlDetail,
  textDetail,
} from "./body-fixtures";
import { fixtureCache } from "./fixture-cache";
import { FASTMAIL, FIXED_NOW, THREAD, THREAD_ID } from "./fixtures";
import { MessageCard } from "./message-card";
import { startOfDay } from "./row-time";
import { routed, seedMail } from "./story-router";
import type { PlannedMessage } from "./thread-messages";
import { useInspect } from "./use-inspect";

const NEWEST_ID = "e-3";
const TODAY = startOfDay(FIXED_NOW);

function newest(): PlannedMessage {
  const email = new Map(Object.entries(THREAD.emails)).get(NEWEST_ID);
  if (email === undefined) {
    throw new Error("the fixture thread has no newest message");
  }
  return { email, unread: false, expanded: true, older: false };
}

function nothing(): void {
  // A drawn card folds nothing and marks nothing.
}

// One open card showing the body, its details opening as in a thread.
function OpenCard({ body }: { body: BodyDetail }): JSX.Element {
  const cache = fixtureCache({}, { [THREAD_ID]: THREAD }, { [NEWEST_ID]: body });
  const listRef = useRef<HTMLOListElement>(null);
  const inspect = useInspect(listRef, "en", true);
  return (
    <ol ref={listRef} role="list" style={{ margin: 0, padding: 0, listStyle: "none" }}>
      <MessageCard
        locale="en"
        today={TODAY}
        message={newest()}
        expanded
        cache={cache}
        accountId={FASTMAIL.id}
        inspect={inspect.of(NEWEST_ID)}
        onToggle={nothing}
      />
    </ol>
  );
}

// The card under the reader's preferences, with what it reads in place.
function card(body: BodyDetail, preferences: Preferences = {}): JSX.Element {
  return routed(
    () => <OpenCard body={body} />,
    undefined,
    seedMail(new Map([[NEWEST_ID, body]]), preferences),
  );
}

const meta: Meta = {
  title: "Mail/MessageCard",
};

export default meta;

export const Newsletter: StoryObj = {
  render: () => card(htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED })),
};

// In the dark theme the mail stands as sent on its white sheet and the
// revert control shows pressed; in the light theme nothing differs.
export const ShownAsSent: StoryObj = {
  render: () =>
    card(htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }), {
      darkMail: "original",
    }),
};

// The largest type and the loosest leading: the frame's text follows the card's.
export const LargerAndLoose: StoryObj = {
  globals: { fontSize: "larger", lineHeight: "loose" },
  render: () => card(htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED })),
};

export const Reply: StoryObj = {
  render: () => card(textDetail(NEWEST_ID, REPLY_TEXT, { flowed: { delSp: false } })),
};

// The files alone: no route serves a story, so a photo would fall back
// to its chip once its request failed.
export const Attachments: StoryObj = {
  render: () =>
    card(
      attachmentsDetail(
        NEWEST_ID,
        ATTACHMENT_PARTS.filter((one) => one !== PHOTO_PART),
      ),
    ),
};
