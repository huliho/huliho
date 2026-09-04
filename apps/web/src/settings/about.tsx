// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { BrandMark } from "../design-system/brand-mark";
import { LegalNotices } from "../legal/legal-notices";
import { getLocale } from "../paraglide/runtime.js";
import { SettingsSection } from "./settings-section";

export function AboutSettings() {
  const locale = getLocale();
  return (
    <>
      <SettingsSection>
        <BrandMark />
      </SettingsSection>
      <SettingsSection title="Huliho">
        <LegalNotices locale={locale} align="start" />
      </SettingsSection>
    </>
  );
}
