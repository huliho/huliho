// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Authentication } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { PASSED, REPLY_TEXT } from "../body-fixtures";
import { DOWNLOAD_PREFIX } from "../body/frame-rig";
import { PlainText } from "../plain-text";
import { routed } from "../story-router";
import { MessageInspector } from "./message-inspector";
import type { InspectorTab } from "./message-inspector";

const DOWNLOAD = `${DOWNLOAD_PREFIX}e-3/message.eml?type=message%2Frfc822`;

// The raw message the source tab reads; a story carries it as a data
// address, since no route answers a story.
const SOURCE = [
  "Return-Path: <pieter@blom-installaties.example>",
  "Received: from mail.blom-installaties.example (mail.blom-installaties.example [203.0.113.7])",
  "        by mx.fastmail.example with ESMTPS id 4Hx9k2Lq; Tue, 12 May 2026 08:15:02 +0200",
  "Authentication-Results: mx.fastmail.example;",
  "        spf=pass smtp.mailfrom=blom-installaties.example;",
  "        dkim=pass header.d=blom-installaties.example;",
  "        dmarc=pass header.from=blom-installaties.example",
  "From: Pieter Blom <pieter@blom-installaties.example>",
  "To: Sanne Bakker <sanne@fastmail.com>",
  "Subject: Re: Offerte badkamerrenovatie, herziene versie",
  "Date: Tue, 12 May 2026 08:15:00 +0200",
  "Message-ID: <offerte-v3@blom-installaties.example>",
  "MIME-Version: 1.0",
  "Content-Type: text/plain; charset=utf-8; format=flowed",
  "",
  REPLY_TEXT,
  "",
].join("\r\n");

const SOURCE_URL = `data:message/rfc822,${encodeURIComponent(SOURCE)}`;

// A header that names no server, as Outlook.com writes it.
const OWN_SERVER: Authentication = {
  status: "parsed",
  results: {
    server: null,
    spf: "none",
    dkim: "unknown",
    dmarc: "fail",
    dmarcFrom: "blom-installaties.example",
  },
};

const LINKS = { ownHost: "mail.example.test", linkKey: "key-1" };

function nothing(): void {
  // A drawn dialog stays open.
}

interface StoryFacts {
  authentication: Authentication;
  plain: boolean;
  tab: InspectorTab;
}

// The inspector over the reply of the fixture thread.
function Inspector({ authentication, plain, tab }: StoryFacts): JSX.Element {
  const text = <PlainText text={REPLY_TEXT} flowed={{ delSp: false }} links={LINKS} />;
  return (
    <MessageInspector
      locale="en"
      open
      onClose={nothing}
      onClosed={nothing}
      opener={undefined}
      rendered={text}
      body={{
        authentication,
        plain: plain ? { text, short: null } : null,
        download: DOWNLOAD,
        source: { key: ["story", "source", tab], url: SOURCE_URL },
      }}
      initialTab={tab}
    />
  );
}

function story(facts: StoryFacts): JSX.Element {
  return routed(() => <Inspector {...facts} />);
}

const meta: Meta = {
  title: "Mail/MessageInspector",
};

export default meta;

export const Rendered: StoryObj = {
  render: () => story({ authentication: PASSED, plain: true, tab: "rendered" }),
};

// A message without a text part, checked by a server that names itself nothing.
export const PlainMissing: StoryObj = {
  render: () => story({ authentication: OWN_SERVER, plain: false, tab: "plain" }),
};

export const Source: StoryObj = {
  render: () => story({ authentication: { status: "absent" }, plain: true, tab: "source" }),
};
