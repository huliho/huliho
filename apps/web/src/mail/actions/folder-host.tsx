// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use } from "react";

import { Button } from "../../design-system/button";
import { Dialog, DialogActions } from "../../design-system/dialog";
import { Field } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { PaneBoundary, PaneErrorState } from "../../shell/pane-boundary";
import { prefetchMenus } from "../menus-chunk";
import { closeFolderAsk, settleFolderAsk, useFolderAsk } from "./folder-asks";
import type { Asked, FolderAsk } from "./folder-asks";

// The parts the dialogs take from the page's start.
export const DIALOG_PARTS = {
  Dialog,
  DialogActions,
  Field,
  Button,
  close: closeFolderAsk,
  settle: settleFolderAsk,
};

function LoadedDialogs({ locale, asked }: { locale: Locale; asked: Asked }) {
  const { FolderDialogs } = use(prefetchMenus());
  return <FolderDialogs locale={locale} asked={asked} parts={DIALOG_PARTS} />;
}

// The name a dialog that could not load carries, as its item words it.
function titleOf(ask: FolderAsk, locale: Locale): string {
  switch (ask.kind) {
    case "create":
      return m.command_folder({ kind: "new" }, { locale });
    case "empty":
      return m.folder_empty({ role: ask.mailbox.role ?? "" }, { locale });
    default:
      return m.command_folder({ kind: ask.kind }, { locale });
  }
}

// The folder dialogs ride the menus chunk and stand only while one is
// asked; a refused import shows the pane's sentence with Try again in a
// dialog of the same name.
export function FolderHost({ locale }: { locale: Locale }) {
  const asked = useFolderAsk();
  const { ask, open } = asked;
  if (ask === null) {
    return null;
  }
  return (
    <PaneBoundary
      frame={
        <Dialog
          open={open}
          onOpenChange={(next) => {
            if (!next) {
              closeFolderAsk();
            }
          }}
          onClosed={settleFolderAsk}
          title={titleOf(ask, locale)}
        >
          <PaneErrorState />
        </Dialog>
      }
    >
      <Suspense fallback={null}>
        <LoadedDialogs locale={locale} asked={asked} />
      </Suspense>
    </PaneBoundary>
  );
}
