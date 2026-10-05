// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { openHref } from "./fragment";
import { linkKey } from "./link-key";
import { OpenLinkPage } from "./open-link";

function nothing(): void {
  // Stories render states; nothing opens and no tab closes.
}

// The page for one link; `made` says whether this device made it.
function page(target: string, text: string, made = true): JSX.Element {
  const address = openHref({ target, text, key: made ? linkKey() : "" }) ?? "";
  const tab = {
    hash: address.slice(address.indexOf("#")),
    hostname: "mail.example.test",
    replace: nothing,
    hand: nothing,
    close: nothing,
  };
  return <OpenLinkPage tab={tab} />;
}

const meta: Meta = {
  title: "Open/Link",
};

export default meta;

export const TextNamesAnotherHost: StoryObj = {
  render: () =>
    page(
      "https://mybank-secure.example.net/login/verify?session=8f3a2c91&return=%2Faccount",
      "mybank.example",
    ),
};

export const InternationalDomain: StoryObj = {
  render: () => page("https://xn--mybnk-fsa.example/inloggen", "Sign in"),
};

export const OwnMailApp: StoryObj = {
  render: () => page("https://mail.example.test/settings/accounts", "Settings"),
};

export const MadeElsewhere: StoryObj = {
  render: () => page("https://shop.example/sale", "", false),
};

export const CannotBeOpened: StoryObj = {
  render: () => page("javascript:top.__x=1", ""),
};

export const Opening: StoryObj = {
  render: () => page("https://shop.example/sale", "See the sale"),
};

export const OpeningMail: StoryObj = {
  render: () => page("mailto:sanne@example.test", "Write to Sanne"),
};
