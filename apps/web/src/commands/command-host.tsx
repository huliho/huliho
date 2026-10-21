// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useEffect, useRef, useState } from "react";

import { Dialog } from "../design-system/dialog";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { chunk } from "../shell/chunk";
import { PaneBoundary, PaneErrorState } from "../shell/pane-boundary";
import type { Chord } from "./keys";
import type { MoveMode } from "./move-sections";
import { noteRecent, recentCommands } from "./recent";
import { awaitLayer } from "./registry";
import type { Command } from "./registry";
import { useCommand, useRegistered } from "./use-command";

// The palette and the overlay come as one chunk of their own, outside
// the initial bundle; the host fetches it on mount, so a key finds it in.
export const prefetchOverlays = chunk(() => import("./overlays"));

const PALETTE_KEYS: readonly Chord[] = [{ key: "k", mod: true }];
const SHORTCUTS_KEYS: readonly Chord[] = [{ key: "?" }];

type Surface = "palette" | "shortcuts" | "move";

interface SurfaceProps {
  surface: Surface;
  open: boolean;
  recent: readonly string[];
  move: MoveMode | null;
  onOpenChange: (open: boolean) => void;
  onClosed: () => void;
  onRun: (command: Command) => void;
  onPick: (id: string) => void;
}

// The one surface open at a time and what happens around it.
interface Surfaces {
  // The surface in the tree, kept through its closing fade.
  mounted: Surface | null;
  open: boolean;
  recent: readonly string[];
  move: MoveMode | null;
  show: (surface: Surface, move?: MoveMode) => void;
  onOpenChange: (open: boolean) => void;
  onClosed: () => void;
  onRun: (command: Command) => void;
  onPick: (id: string) => void;
}

let showMove: ((mode: MoveMode) => void) | null = null;

// Opens the palette in its move mode; nothing while no host stands.
export function openMoveMode(mode: MoveMode): void {
  showMove?.(mode);
}

function LoadedSurface(props: SurfaceProps) {
  const { surface, open, recent, onOpenChange, onClosed, onRun } = props;
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
      move={surface === "move" ? (props.move ?? undefined) : undefined}
      onPick={props.onPick}
    />
  );
}

const TITLES: Record<Surface, (locale: Locale) => string> = {
  palette: (locale) => m.command_palette({}, { locale }),
  shortcuts: (locale) => m.shortcuts_title({}, { locale }),
  move: (locale) => m.command_thread({ action: "move" }, { locale }),
};

// A refused chunk shows its error in a dialog named as the surface, so
// Escape closes it and the focus stays in a layer.
function Surface(props: SurfaceProps) {
  const { open, onOpenChange, onClosed } = props;
  const locale = useLocale();
  const title = TITLES[props.surface](locale);
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
  const [move, setMove] = useState<MoveMode | null>(null);
  const pending = useRef<(() => void) | null>(null);
  const origin = useRef<Element | null>(null);
  const close = (): void => {
    setOpen(false);
    const from = origin.current;
    if (from instanceof HTMLElement && from.isConnected) {
      from.focus();
    }
  };
  const runPending = (): void => {
    const run = pending.current;
    pending.current = null;
    run?.();
  };
  return {
    mounted,
    open,
    recent,
    move,
    show: (surface, mode) => {
      runPending();
      awaitLayer();
      origin.current = document.activeElement;
      setRecent(recentCommands());
      setMove(mode ?? null);
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
      setMove(null);
      runPending();
    },
    onRun: (command) => {
      pending.current = command.run;
      noteRecent(command.id);
      close();
    },
    onPick: (id) => {
      const picked = move;
      pending.current = () => {
        picked?.onPick(id);
      };
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
  // Each render lends its own opener, so the move mode starts from the
  // surfaces as they stand.
  useEffect(() => {
    const opener = (mode: MoveMode): void => {
      surfaces.show("move", mode);
    };
    showMove = opener;
    return () => {
      if (showMove === opener) {
        showMove = null;
      }
    };
  });
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
      move={surfaces.move}
      onOpenChange={surfaces.onOpenChange}
      onClosed={surfaces.onClosed}
      onRun={surfaces.onRun}
      onPick={surfaces.onPick}
    />
  );
}
