// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useEffect, useRef, useState } from "react";

import { Dialog } from "../design-system/dialog";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import { chunk } from "../shell/chunk";
import { PaneBoundary, PaneErrorState } from "../shell/pane-boundary";
import type { Chord } from "./keys";
import { noteRecent, recentCommands } from "./recent";
import type { Command } from "./registry";
import { useCommand, useRegistered } from "./use-command";

// The palette and the overlay come as one chunk of their own, outside
// the initial bundle; the host fetches it on mount, so a key finds it in.
export const prefetchOverlays = chunk(() => import("./overlays"));

const PALETTE_KEYS: readonly Chord[] = [{ key: "k", mod: true }];
const SHORTCUTS_KEYS: readonly Chord[] = [{ key: "?" }];

type Surface = "palette" | "shortcuts";

interface SurfaceProps {
  surface: Surface;
  open: boolean;
  recent: readonly string[];
  onOpenChange: (open: boolean) => void;
  onClosed: () => void;
  onRun: (command: Command) => void;
}

// The one surface open at a time and what happens around it.
interface Surfaces {
  // The surface in the tree, kept through its closing fade.
  mounted: Surface | null;
  open: boolean;
  recent: readonly string[];
  show: (surface: Surface) => void;
  onOpenChange: (open: boolean) => void;
  onClosed: () => void;
  onRun: (command: Command) => void;
}

function LoadedSurface({ surface, open, recent, onOpenChange, onClosed, onRun }: SurfaceProps) {
  const { CommandPalette, ShortcutOverlay } = use(prefetchOverlays());
  const locale = useLocale();
  const commands = useRegistered();
  if (surface === "shortcuts") {
    return (
      <ShortcutOverlay
        open={open}
        onOpenChange={onOpenChange}
        onClosed={onClosed}
        locale={locale}
        commands={commands}
      />
    );
  }
  return (
    <CommandPalette
      open={open}
      onOpenChange={onOpenChange}
      onClosed={onClosed}
      locale={locale}
      commands={commands}
      recent={recent}
      onRun={onRun}
    />
  );
}

// A refused chunk shows its error in a dialog named as the surface, so
// Escape closes it and the focus stays in a layer.
function Surface(props: SurfaceProps) {
  const { surface, open, onOpenChange, onClosed } = props;
  const locale = useLocale();
  const title =
    surface === "shortcuts" ? m.shortcuts_title({}, { locale }) : m.command_palette({}, { locale });
  return (
    <PaneBoundary
      frame={
        <Dialog open={open} onOpenChange={onOpenChange} onClosed={onClosed} title={title}>
          <PaneErrorState />
        </Dialog>
      }
    >
      <Suspense fallback={null}>
        <LoadedSurface {...props} />
      </Suspense>
    </PaneBoundary>
  );
}

// The focus goes back to where the key was pressed the moment a surface
// closes, ahead of its fade. A picked command runs once the fade has
// ended, so it starts from that focus; a key that opens a surface
// during that fade runs it first, from the same focus, so it never
// runs after the other surface.
function useSurfaces(): Surfaces {
  const [mounted, setMounted] = useState<Surface | null>(null);
  const [open, setOpen] = useState(false);
  const [recent, setRecent] = useState<readonly string[]>([]);
  const pending = useRef<Command | null>(null);
  const origin = useRef<Element | null>(null);
  const close = (): void => {
    setOpen(false);
    const from = origin.current;
    if (from instanceof HTMLElement && from.isConnected) {
      from.focus();
    }
  };
  const runPending = (): void => {
    const command = pending.current;
    pending.current = null;
    command?.run();
  };
  return {
    mounted,
    open,
    recent,
    show: (surface) => {
      runPending();
      origin.current = document.activeElement;
      setRecent(recentCommands());
      setMounted(surface);
      setOpen(true);
    },
    onOpenChange: (next) => {
      if (next) {
        setOpen(true);
      } else {
        close();
      }
    },
    onClosed: () => {
      setMounted(null);
      runPending();
    },
    onRun: (command) => {
      pending.current = command;
      noteRecent(command.id);
      close();
    },
  };
}

// The two commands that open a surface over the registry, behind every
// signed-in screen.
export function CommandHost() {
  const locale = useLocale();
  const surfaces = useSurfaces();
  useEffect(() => {
    void prefetchOverlays();
  }, []);
  useCommand({
    id: "palette.open",
    label: m.command_palette({}, { locale }),
    group: "app",
    keys: PALETTE_KEYS,
    run: () => {
      surfaces.show("palette");
    },
  });
  useCommand({
    id: "shortcuts.open",
    label: m.command_shortcuts({}, { locale }),
    group: "app",
    keys: SHORTCUTS_KEYS,
    run: () => {
      surfaces.show("shortcuts");
    },
  });
  if (surfaces.mounted === null) {
    return null;
  }
  return (
    <Surface
      surface={surfaces.mounted}
      open={surfaces.open}
      recent={surfaces.recent}
      onOpenChange={surfaces.onOpenChange}
      onClosed={surfaces.onClosed}
      onRun={surfaces.onRun}
    />
  );
}
