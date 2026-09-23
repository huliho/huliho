// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Density, PreferenceChange, Preferences, ReadingPane, Theme } from "@huliho/core";
import { localeEndonym } from "@huliho/i18n";
import { useId } from "react";

import { RadioGroup } from "../../design-system/radio-group";
import type { RadioOption } from "../../design-system/radio-group";
import { listedLocales } from "../../i18n/locale";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { DEFAULT_APPEARANCE } from "../../theme/appearance";
import { SettingsSection } from "../settings-section";
import styles from "./appearance.module.css";

// Where a conversation opens until the user says otherwise.
const DEFAULT_READING_PANE: ReadingPane = "right";

function themeOptions(locale: Locale): RadioOption<Theme>[] {
  return [
    { value: "system", label: m.appearance_theme_system({}, { locale }) },
    { value: "light", label: m.appearance_theme_light({}, { locale }) },
    { value: "dark", label: m.appearance_theme_dark({}, { locale }) },
  ];
}

function densityOptions(locale: Locale): RadioOption<Density>[] {
  return [
    { value: "comfortable", label: m.appearance_density_comfortable({}, { locale }) },
    { value: "compact", label: m.appearance_density_compact({}, { locale }) },
  ];
}

function readingPaneOptions(locale: Locale): RadioOption<ReadingPane>[] {
  return [
    { value: "right", label: m.appearance_reading_pane_right({}, { locale }) },
    { value: "bottom", label: m.appearance_reading_pane_bottom({}, { locale }) },
    { value: "off", label: m.appearance_reading_pane_off({}, { locale }) },
  ];
}

function localeOptions(locale: Locale): RadioOption<Locale>[] {
  return listedLocales(locale).map((listed) => ({ value: listed, label: localeEndonym(listed) }));
}

interface SettingProps<T extends string> {
  title: string;
  hint?: string;
  options: RadioOption<T>[];
  value: T;
  onChange: (value: T) => void;
}

// One card per setting: its name, the control and a sentence where one helps.
function Setting<T extends string>({ title, hint, ...choice }: SettingProps<T>) {
  const titleId = useId();
  const hintId = useId();
  return (
    <SettingsSection title={title} titleId={titleId}>
      <RadioGroup
        labelledBy={titleId}
        describedBy={hint === undefined ? undefined : hintId}
        {...choice}
      />
      {hint !== undefined && (
        <p id={hintId} className={styles.hint}>
          {hint}
        </p>
      )}
    </SettingsSection>
  );
}

export interface AppearanceFormProps {
  locale: Locale;
  preferences: Preferences;
  onChange: (change: PreferenceChange) => void;
  onSwitchLocale: (locale: Locale) => void;
}

export function AppearanceForm({
  locale,
  preferences,
  onChange,
  onSwitchLocale,
}: AppearanceFormProps) {
  return (
    <>
      <Setting
        title={m.appearance_theme({}, { locale })}
        options={themeOptions(locale)}
        value={preferences.theme ?? DEFAULT_APPEARANCE.theme}
        onChange={(value) => {
          onChange({ key: "theme", value });
        }}
      />
      <Setting
        title={m.appearance_density({}, { locale })}
        hint={m.appearance_density_hint({}, { locale })}
        options={densityOptions(locale)}
        value={preferences.density ?? DEFAULT_APPEARANCE.density}
        onChange={(value) => {
          onChange({ key: "density", value });
        }}
      />
      <Setting
        title={m.appearance_reading_pane({}, { locale })}
        hint={m.appearance_reading_pane_hint({}, { locale })}
        options={readingPaneOptions(locale)}
        value={preferences.readingPane ?? DEFAULT_READING_PANE}
        onChange={(value) => {
          onChange({ key: "readingPane", value });
        }}
      />
      <Setting
        title={m.locale_label({}, { locale })}
        options={localeOptions(locale)}
        value={locale}
        onChange={onSwitchLocale}
      />
    </>
  );
}
