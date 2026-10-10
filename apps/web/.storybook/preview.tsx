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
// The font size and the line height at their own step.
const FALLBACK_STEP = "default";

function globalString(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

// The attributes the document wears in the app, from the toolbar's globals.
interface Worn {
  theme: string;
  density: string;
  fontSize: string;
  lineHeight: string;
}

function GlobalAttributes({ theme, density, fontSize, lineHeight }: Worn): null {
  useEffect(() => {
    document.documentElement.lang = "en";
    document.documentElement.dataset["theme"] = theme;
    document.documentElement.dataset["density"] = density;
    document.documentElement.dataset["fontSize"] = fontSize;
    document.documentElement.dataset["lineHeight"] = lineHeight;
  }, [theme, density, fontSize, lineHeight]);
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
    fontSize: {
      description: "Font size",
      toolbar: { title: "Font size", items: ["default", "large", "larger"] },
    },
    lineHeight: {
      description: "Line height",
      toolbar: { title: "Line height", items: ["default", "relaxed", "loose"] },
    },
  },
  initialGlobals: {
    theme: FALLBACK_THEME,
    density: FALLBACK_DENSITY,
    fontSize: FALLBACK_STEP,
    lineHeight: FALLBACK_STEP,
  },
  decorators: [
    (Story, context) => (
      <>
        <GlobalAttributes
          theme={globalString(context.globals["theme"], FALLBACK_THEME)}
          density={globalString(context.globals["density"], FALLBACK_DENSITY)}
          fontSize={globalString(context.globals["fontSize"], FALLBACK_STEP)}
          lineHeight={globalString(context.globals["lineHeight"], FALLBACK_STEP)}
        />
        <Story />
      </>
    ),
  ],
};

export default preview;
