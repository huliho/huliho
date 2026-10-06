// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { ATTACHMENTS_TEXT, attachmentsDetail } from "../body-fixtures";
import bodyStyles from "../message-body.module.css";
import cardStyles from "../message-card.module.css";
import { PreviewDialog, WarningDialog } from "./attachment-dialogs";
import { AttachmentStrip } from "./attachment-strip";
import { attachmentsOf } from "./parts";
import type { Attachment } from "./parts";

// A photo of 480 by 360 the story carries itself, where no route answers.
const PHOTO =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAeAAAAFoCAIAAAAAVb93AAAD5klEQVR42u3UQQ0AIAwAsfkXgA6eE4AGBKBkNrbQpArucZH3AdBQSABg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGLQKAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNIBBqwBg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDYNAABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAYNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBAxg0AAYNgEEDGDQABg1g0AAYNAAGDWDQABg0gEEDYNAABg2AQQNg0AAGDYBBA3ww6LMXAA0ZNIBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg1g0BIAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQNg0AAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQABg0gEEDYNAABg2AQQMYNAAGDYBBAxg0AAYNYNAAGDQABg1g0AAYNIBBA2DQAAatAoBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0AAYNYNAAGDSAQQNg0AAGDYBBA2DQAAYNgEEDGDQABg2AQQMYNAAGDWDQABg0gEEDYNAAGDSAQQNg0AAGDYBBA2DQAEMUn6QFLdA47G8AAAAASUVORK5CYII=";

function nothing(): void {
  // A drawn dialog stays open.
}

// A pictured part as a copy with the photo from the story; any other part as it is.
function fromStory(one: Attachment): Attachment {
  return one.preview === null ? one : { ...one, preview: PHOTO };
}

// The fixture's attachments, the photo shown from the story itself.
const ATTACHMENTS: Attachment[] = attachmentsOf(attachmentsDetail("e-3"), new Set(), "en").map(
  fromStory,
);
const PHOTO_ATTACHMENT = ATTACHMENTS.find((one) => one.preview !== null) ?? ATTACHMENTS[0];
const DANGEROUS_ATTACHMENT = ATTACHMENTS.find((one) => one.dangerous) ?? ATTACHMENTS[0];

// The strip under a short body, in a card.
function Strip(): JSX.Element {
  return (
    <ol role="list" style={{ margin: 0, padding: 0, listStyle: "none" }}>
      <li className={cardStyles.card} data-expanded>
        <div className={cardStyles.body} data-strip>
          <p className={bodyStyles.sentence}>{ATTACHMENTS_TEXT}</p>
        </div>
        <AttachmentStrip locale="en" attachments={ATTACHMENTS} />
      </li>
    </ol>
  );
}

const meta: Meta = {
  title: "Mail/AttachmentStrip",
};

export default meta;

export const Default: StoryObj = {
  render: () => <Strip />,
};

export const PreviewOpen: StoryObj = {
  render: () => (
    <>
      <Strip />
      {PHOTO_ATTACHMENT !== undefined && (
        <PreviewDialog
          locale="en"
          opened={{ attachment: PHOTO_ATTACHMENT, opener: document.body }}
          open
          onClose={nothing}
          onClosed={nothing}
        />
      )}
    </>
  ),
};

export const WarningOpen: StoryObj = {
  render: () => (
    <>
      <Strip />
      {DANGEROUS_ATTACHMENT !== undefined && (
        <WarningDialog
          locale="en"
          opened={{ attachment: DANGEROUS_ATTACHMENT, opener: document.body }}
          open
          onClose={nothing}
          onClosed={nothing}
        />
      )}
    </>
  ),
};
