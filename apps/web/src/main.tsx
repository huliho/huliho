// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/base.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app";
import { loadInstanceOverride } from "./theme/instance-override";

async function applyInstanceOverride(): Promise<void> {
  const result = await loadInstanceOverride(document);
  if (!result.applied && result.message !== undefined) {
    console.error(`instance override: ${result.message}`);
  }
}
void applyInstanceOverride();

const container = document.getElementById("root");
if (container === null) {
  throw new Error("the root element is missing from index.html");
}
createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
