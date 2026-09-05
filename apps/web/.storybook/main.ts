// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { StorybookConfig } from "@storybook/react-vite";
import type { Plugin } from "vite";

// The strict execution order Storybook's mocker asks of rolldown breaks zod's internal cycle.
const defaultExecutionOrder: Plugin = {
  name: "huliho-default-execution-order",
  enforce: "post",
  config: (viteConfig) => {
    const output = viteConfig.build?.rolldownOptions?.output;
    for (const options of Array.isArray(output) ? output : [output]) {
      if (options !== undefined) {
        options.strictExecutionOrder = false;
      }
    }
  },
};

const config: StorybookConfig = {
  framework: { name: "@storybook/react-vite", options: {} },
  stories: ["../src/**/*.stories.tsx"],
  core: { disableTelemetry: true },
  viteFinal: (viteConfig) => ({
    ...viteConfig,
    plugins: [...(viteConfig.plugins ?? []), defaultExecutionOrder],
  }),
};

export default config;
