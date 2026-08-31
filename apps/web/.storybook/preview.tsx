// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import "../src/styles/fonts.css";
import "../src/styles/tokens.css";
import "../src/styles/base.css";

import type { Preview } from "@storybook/react-vite";
import { useEffect } from "react";

const FALLBACK_THEME = "light";
const FALLBACK_DENSITY = "comfortable";

function globalString(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

function GlobalAttributes({ theme, density }: { theme: string; density: string }): null {
  useEffect(() => {
    document.documentElement.lang = "en";
    document.documentElement.dataset["theme"] = theme;
    document.documentElement.dataset["density"] = density;
  }, [theme, density]);
  return null;
}

const preview: Preview = {
  globalTypes: {
    theme: {
      description: "Color theme",
      toolbar: { title: "Theme", items: ["light", "dark"] },
    },
    density: {
      description: "Density mode",
      toolbar: { title: "Density", items: ["comfortable", "compact", "touch"] },
    },
  },
  initialGlobals: { theme: FALLBACK_THEME, density: FALLBACK_DENSITY },
  decorators: [
    (Story, context) => (
      <>
        <GlobalAttributes
          theme={globalString(context.globals["theme"], FALLBACK_THEME)}
          density={globalString(context.globals["density"], FALLBACK_DENSITY)}
        />
        <Story />
      </>
    ),
  ],
};

export default preview;
