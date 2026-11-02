// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ChangeEvent } from "react";
import { useId } from "react";

import { LeavingButton, focusHeir } from "../../design-system/button";
import { Field } from "../../design-system/field";
import { RadioGroup } from "../../design-system/radio-group";
import type { RadioOption } from "../../design-system/radio-group";
import rowList from "../../design-system/row-list.module.css";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { SettingsSection } from "../settings-section";
import type { ClosedFolder, FolderChoice, RoleName, RoleRow } from "./folder-choices";
import type { PendingWrite } from "./use-folder-writes";
import styles from "./folder-sections.module.css";

// The select's value for no mailbox.
const NO_MAILBOX = "";

export type ClosedChoice = "link" | "hide";

// The control a write came from, for the value it shows meanwhile.
export function roleControl(role: RoleName): string {
  return `role:${role}`;
}

export function showControl(id: string): string {
  return `show:${id}`;
}

export function closedControl(id: string): string {
  return `closed:${id}`;
}

interface RoleFieldProps {
  locale: Locale;
  row: RoleRow;
  online: boolean;
  pending: PendingWrite | null;
  onRole: (role: RoleName, takerId: string | null) => void;
}

function RoleField({ locale, row, online, pending, onRole }: RoleFieldProps) {
  const control = roleControl(row.role);
  const value = pending?.control === control ? pending.value : (row.holderId ?? NO_MAILBOX);
  return (
    <Field
      label={m.folders_role({ role: row.role }, { locale })}
      value={value}
      disabled={row.fixed || !online}
      aria-busy={pending?.control === control || undefined}
      onChange={(event: ChangeEvent<HTMLInputElement>) => {
        onRole(row.role, event.target.value === NO_MAILBOX ? null : event.target.value);
      }}
    >
      {!row.fixed && <option value={NO_MAILBOX}>{m.folders_none({}, { locale })}</option>}
      {row.choices.map((choice) => (
        <option key={choice.id} value={choice.id}>
          {choice.label}
        </option>
      ))}
    </Field>
  );
}

export interface RolesSectionProps {
  locale: Locale;
  name: string;
  rows: RoleRow[];
  online: boolean;
  pending: PendingWrite | null;
  onRole: (role: RoleName, takerId: string | null) => void;
}

// The account's name and its six roles. The inbox stays where the server
// keeps it on every account, so a row of its own needs no sentence.
export function RolesSection({ locale, name, rows, online, pending, onRole }: RolesSectionProps) {
  const serverKeeps = rows.some((row) => row.fixed && row.role !== "inbox");
  return (
    <SettingsSection title={name}>
      {!online && (
        <p className={styles.hint}>{m.folder_refused({ reason: "offline" }, { locale })}</p>
      )}
      <div className={styles.roles}>
        {rows.map((row) => (
          <RoleField
            key={row.role}
            locale={locale}
            row={row}
            online={online}
            pending={pending}
            onRole={onRole}
          />
        ))}
      </div>
      {serverKeeps && <p className={styles.hint}>{m.folders_server_keeps({}, { locale })}</p>}
    </SettingsSection>
  );
}

export interface HiddenSectionProps {
  locale: Locale;
  folders: FolderChoice[];
  online: boolean;
  pending: PendingWrite | null;
  onShow: (id: string) => void;
}

// The folders the tree leaves out. Show takes its row away, so the box
// around the list takes the focus it held.
export function HiddenSection({ locale, folders, online, pending, onShow }: HiddenSectionProps) {
  const title = m.folders_hidden({}, { locale });
  return (
    <SettingsSection title={title}>
      <div {...focusHeir}>
        {folders.length === 0 ? (
          <p className={styles.hint}>{m.folders_no_hidden({}, { locale })}</p>
        ) : (
          <ul className={rowList.list} aria-label={title}>
            {folders.map((folder) => (
              <li key={folder.id} className={rowList.row}>
                <span className={styles.name}>
                  <bdi>{folder.label}</bdi>
                </span>
                <LeavingButton
                  aria-label={m.folders_show_for({ name: folder.label }, { locale })}
                  held={!online}
                  pending={pending?.control === showControl(folder.id)}
                  onClick={() => {
                    onShow(folder.id);
                  }}
                >
                  {m.folders_show({}, { locale })}
                </LeavingButton>
              </li>
            ))}
          </ul>
        )}
      </div>
    </SettingsSection>
  );
}

function closedOptions(locale: Locale): RadioOption<ClosedChoice>[] {
  return [
    { value: "link", label: m.folders_as_link({}, { locale }) },
    { value: "hide", label: m.folder_hide({}, { locale }) },
  ];
}

interface ClosedRowProps {
  locale: Locale;
  folder: ClosedFolder;
  online: boolean;
  pending: PendingWrite | null;
  onClosed: (id: string, choice: ClosedChoice) => void;
}

function ClosedRow({ locale, folder, online, pending, onClosed }: ClosedRowProps) {
  const nameId = useId();
  const control = closedControl(folder.id);
  const hidden = pending?.control === control ? pending.value === "hide" : folder.hidden;
  return (
    <li className={styles.closed}>
      <span id={nameId} className={styles.name}>
        <bdi>{folder.label}</bdi>
      </span>
      <RadioGroup
        labelledBy={nameId}
        disabled={!online}
        options={closedOptions(locale)}
        value={hidden ? "hide" : "link"}
        onChange={(next) => {
          onClosed(folder.id, next);
        }}
      />
    </li>
  );
}

export interface ClosedSectionProps {
  locale: Locale;
  folders: ClosedFolder[];
  online: boolean;
  pending: PendingWrite | null;
  onClosed: (id: string, choice: ClosedChoice) => void;
}

// The mailboxes the server does not open: an empty row in the tree or none.
export function ClosedSection({ locale, folders, online, pending, onClosed }: ClosedSectionProps) {
  const title = m.folders_closed({}, { locale });
  return (
    <SettingsSection title={title}>
      <ul className={styles.closedList} aria-label={title}>
        {folders.map((folder) => (
          <ClosedRow
            key={folder.id}
            locale={locale}
            folder={folder}
            online={online}
            pending={pending}
            onClosed={onClosed}
          />
        ))}
      </ul>
    </SettingsSection>
  );
}
