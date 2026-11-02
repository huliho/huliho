// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";

import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import type { Locale } from "../../paraglide/runtime.js";
import { SettingsSection } from "../settings-section";
import { ROLES, closedFolders, hiddenFolders, roleRows } from "./folder-choices";
import { FOLDERS, closedBox, fixedBox, folderBox, hiddenBox } from "./folder-fixtures";
import { ClosedSection, HiddenSection, RolesSection, roleControl } from "./folder-sections";
import type { PendingWrite } from "./use-folder-writes";

function nothing(): void {
  // A drawn screen writes nothing.
}

const ACCOUNT_NAME = "sanne@dekker-mail.nl";

const LONG_ID = "mb-lang";

// A long name stays inside its select, its row and the screen.
const WITH_CHOICES: Mailbox[] = [
  ...FOLDERS,
  hiddenBox("mb-kelder", "Kelder"),
  hiddenBox("mb-oud", "Oud", "mb-kelder"),
  folderBox(LONG_ID, "Offertes van onderaannemers voor de verbouwing van de zolder en de kelder"),
  closedBox("mb-gedeeld", "Gedeeld", { subscribed: true }),
  closedBox("mb-lijsten", "Lijsten"),
];

// An account whose role mailboxes the server keeps for itself, as Gmail does.
const FIXED: Mailbox[] = [
  fixedBox("mb-inbox", "Inbox", "inbox"),
  fixedBox("mb-drafts", "Drafts", "drafts"),
  fixedBox("mb-sent", "Sent Mail", "sent"),
  fixedBox("mb-archive", "All Mail", "archive"),
  fixedBox("mb-junk", "Spam", "junk"),
  fixedBox("mb-trash", "Trash", "trash"),
  folderBox("mb-facturen", "Facturen"),
];

interface ScreenProps {
  locale?: Locale;
  mailboxes?: readonly Mailbox[];
  online?: boolean;
  pending?: PendingWrite | null;
}

function Screen({
  locale = "en",
  mailboxes = WITH_CHOICES,
  online = true,
  pending = null,
}: ScreenProps) {
  const closed = closedFolders(mailboxes, locale);
  return (
    <>
      <RolesSection
        locale={locale}
        name={ACCOUNT_NAME}
        rows={roleRows(mailboxes, locale)}
        online={online}
        pending={pending}
        onRole={nothing}
      />
      <HiddenSection
        locale={locale}
        folders={hiddenFolders(mailboxes, locale)}
        online={online}
        pending={pending}
        onShow={nothing}
      />
      {closed.length > 0 && (
        <ClosedSection
          locale={locale}
          folders={closed}
          online={online}
          pending={pending}
          onClosed={nothing}
        />
      )}
    </>
  );
}

const meta: Meta = {
  title: "Settings/Folders",
};

export default meta;

// Every role but Junk held, hidden folders and two the server does not open.
export const Default: StoryObj = {
  render: () => <Screen />,
};

export const Dutch: StoryObj = {
  render: () => <Screen locale="nl" />,
};

export const NothingHidden: StoryObj = {
  render: () => <Screen mailboxes={FOLDERS} />,
};

// A server that keeps every role where it is.
export const ServerKeeps: StoryObj = {
  render: () => <Screen mailboxes={FIXED} />,
};

export const ServerKeepsDutch: StoryObj = {
  render: () => <Screen locale="nl" mailboxes={FIXED} />,
};

export const Offline: StoryObj = {
  render: () => <Screen online={false} />,
};

export const OfflineDutch: StoryObj = {
  render: () => <Screen locale="nl" online={false} />,
};

// A pick on its way stands on its row.
export const Moving: StoryObj = {
  render: () => <Screen pending={{ control: roleControl("archive"), value: "mb-facturen" }} />,
};

export const Loading: StoryObj = {
  render: () => (
    <SettingsSection title={ACCOUNT_NAME}>
      <ListSkeleton locale="en" rows={ROLES.length} />
    </SettingsSection>
  ),
};

export const NoMailboxes: StoryObj = {
  render: () => (
    <SettingsSection title={ACCOUNT_NAME}>
      <EmptyState message="This account has no mailboxes to show yet." />
    </SettingsSection>
  ),
};

export const Failed: StoryObj = {
  render: () => (
    <SettingsSection title={ACCOUNT_NAME}>
      <ErrorState
        variant="inline"
        message="Couldn’t load the folders."
        retryLabel="Try again"
        onRetry={nothing}
      />
    </SettingsSection>
  ),
};
