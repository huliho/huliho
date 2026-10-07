// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import type { JSX } from "react";

import { Tabs } from "./tabs";

type View = "rendered" | "plain" | "source";

const TABS = [
  { value: "rendered", label: "Rendered" },
  { value: "plain", label: "Plain text" },
  { value: "source", label: "Source" },
] as const;

// A row of three tabs over a sheet, as the message details lay them out.
function Demo(): JSX.Element {
  const [value, setValue] = useState<View>("rendered");
  return (
    <Tabs
      value={value}
      onValueChange={setValue}
      tabs={TABS}
      panels={{
        rendered: <p style={{ margin: 0 }}>The message as the card renders it.</p>,
        plain: <p style={{ margin: 0 }}>The plain text of the message.</p>,
        source: <p style={{ margin: 0 }}>The raw source of the message.</p>,
      }}
    />
  );
}

const meta: Meta = {
  title: "Primitives/Tabs",
};

export default meta;

export const Default: StoryObj = {
  render: () => <Demo />,
};
