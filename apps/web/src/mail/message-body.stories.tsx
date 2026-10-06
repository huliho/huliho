// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef } from "react";
import type { JSX, RefObject } from "react";

import { focusHeir } from "../design-system/button";
import { useTheme } from "../theme/use-theme";
import { NEWSLETTER_HTML, REPLY_TEXT, htmlDetail } from "./body-fixtures";
import { canAdapt } from "./body/colors";
import { buildFrameDocument } from "./body/frame-document";
import { OPTIONS } from "./body/frame-rig";
import { useFrameStyle } from "./body/use-frame-style";
import { MessageBody } from "./message-body";
import type { BodyView, Cut } from "./message-body";
import styles from "./message-card.module.css";

const LINKS = { ownHost: OPTIONS.ownHost, linkKey: OPTIONS.linkKey };
const TITLE = "Message from De Koersbrief about Week 35: rentes, chips en de bouw";

function nothing(): void {
  // A drawn notice asks for nothing.
}

const DOWNLOAD = "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822";
const CUT_ONCE: Cut = {
  kind: "size",
  size: "4 MB",
  whole: nothing,
  pending: false,
  download: null,
};
const CUT_TWICE: Cut = {
  kind: "size",
  size: "12 MB",
  whole: null,
  pending: false,
  download: DOWNLOAD,
};
const TOO_LONG: Cut = { kind: "lines", download: DOWNLOAD };

interface SlotProps {
  view: BodyView;
  slotRef?: RefObject<HTMLDivElement | null>;
}

// The slot as the card draws it: the body under the hairline, in a card.
function Slot({ view, slotRef }: SlotProps): JSX.Element {
  return (
    <ol role="list" style={{ margin: 0, padding: 0, listStyle: "none" }}>
      <li className={styles.card} data-expanded>
        <div ref={slotRef} {...focusHeir} className={styles.body}>
          <MessageBody locale="en" view={view} />
        </div>
      </li>
    </ol>
  );
}

// The newsletter in the frame, built as a card builds it: in the theme
// the page renders, with the type and colors of the slot around it.
function Newsletter({ cut }: { cut: Cut | null }): JSX.Element {
  const slotRef = useRef<HTMLDivElement>(null);
  const style = useFrameStyle(slotRef);
  const theme = useTheme();
  if (style === null) {
    return <Slot view={{ kind: "loading" }} slotRef={slotRef} />;
  }
  const adapt = theme === "dark" && canAdapt();
  const { html } = buildFrameDocument(htmlDetail("e-3", NEWSLETTER_HTML).body, {
    ...OPTIONS,
    theme,
    adapt,
    style,
  });
  return <Slot view={{ kind: "html", html, title: TITLE, asSent: false, cut }} slotRef={slotRef} />;
}

const meta: Meta = {
  title: "Mail/MessageBody",
};

export default meta;

export const Loading: StoryObj = {
  render: () => <Slot view={{ kind: "loading" }} />,
};

export const Failed: StoryObj = {
  render: () => <Slot view={{ kind: "error", retry: nothing }} />,
};

export const Offline: StoryObj = {
  render: () => <Slot view={{ kind: "offline" }} />,
};

export const Gone: StoryObj = {
  render: () => <Slot view={{ kind: "gone" }} />,
};

export const NoText: StoryObj = {
  render: () => <Slot view={{ kind: "none" }} />,
};

export const TooComplex: StoryObj = {
  render: () => <Slot view={{ kind: "complex" }} />,
};

export const Text: StoryObj = {
  render: () => (
    <Slot view={{ kind: "text", text: REPLY_TEXT, flowed: null, links: LINKS, cut: null }} />
  ),
};

export const TextCut: StoryObj = {
  render: () => (
    <Slot view={{ kind: "text", text: REPLY_TEXT, flowed: null, links: LINKS, cut: CUT_ONCE }} />
  ),
};

export const TextTooLong: StoryObj = {
  render: () => (
    <Slot view={{ kind: "text", text: REPLY_TEXT, flowed: null, links: LINKS, cut: TOO_LONG }} />
  ),
};

export const Html: StoryObj = {
  render: () => <Newsletter cut={null} />,
};

export const HtmlCutTwice: StoryObj = {
  render: () => <Newsletter cut={CUT_TWICE} />,
};
