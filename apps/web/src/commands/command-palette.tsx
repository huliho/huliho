// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import { Dialog } from "@base-ui/react/dialog";
import { FolderInput, Search } from "lucide-react";
import { useRef, useState } from "react";
import type { RefObject } from "react";

import { Kbd } from "../design-system/kbd";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { KeyCaps } from "./key-caps";
import { ESCAPE, chordText } from "./keys";
import { PATH_SEPARATOR, moveSectionsOf } from "./move-sections";
import type { MoveEntry, MoveMode, MoveSection } from "./move-sections";
import { sectionsOf } from "./palette-sections";
import type { Entry, Section } from "./palette-sections";
import type { Command } from "./registry";
import styles from "./command-palette.module.css";

const ARROWS = `${chordText({ key: "ArrowUp" })}${chordText({ key: "ArrowDown" })}`;
const ENTER = chordText({ key: "Enter" });

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // Runs once the closing fade has ended, when the chosen command may run.
  onClosed?: (() => void) | undefined;
  locale: Locale;
  commands: readonly Command[];
  // The ids of the commands last run from here, newest first.
  recent: readonly string[];
  // What the input holds when the palette opens.
  defaultQuery?: string | undefined;
  onRun: (command: Command) => void;
  // The move mode in place of the commands: the mailboxes a move can go
  // to, the pick going to `onPick`.
  move?: MoveMode | undefined;
  onPick?: ((id: string) => void) | undefined;
}

interface BodyProps {
  locale: Locale;
  commands: readonly Command[];
  recent: readonly string[];
  defaultQuery: string;
  inputRef: RefObject<HTMLInputElement | null>;
  onRun: (command: Command) => void;
}

function Row({ entry, onRun }: { entry: Entry; onRun: (command: Command) => void }) {
  return (
    <Autocomplete.Item
      value={entry}
      className={styles.item}
      onClick={() => {
        onRun(entry.command);
      }}
    >
      <span className={styles.label}>{entry.label}</span>
      <KeyCaps keys={entry.command.keys} spoken={false} />
    </Autocomplete.Item>
  );
}

// The input over the list it narrows: the arrow keys move the
// highlight and Enter runs it; the foot names the keys.
function PaletteBody({ locale, commands, recent, defaultQuery, inputRef, onRun }: BodyProps) {
  const [query, setQuery] = useState(defaultQuery);
  const sections = sectionsOf(commands, recent, query, locale);
  return (
    <Autocomplete.Root
      inline
      open
      items={sections}
      filter={null}
      value={query}
      onValueChange={setQuery}
      autoHighlight="always"
      locale={locale}
    >
      <div className={styles.head}>
        <Search className={styles.icon} aria-hidden="true" />
        <Autocomplete.Input
          ref={inputRef}
          className={styles.input}
          placeholder={m.palette_placeholder({}, { locale })}
          aria-label={m.command_palette({}, { locale })}
        />
        <Kbd spoken={false}>{chordText(ESCAPE)}</Kbd>
      </div>
      <Autocomplete.List className={styles.list}>
        {(section: Section) => (
          <Autocomplete.Group key={section.value} items={section.items}>
            <Autocomplete.GroupLabel className={styles.groupLabel}>
              {section.label}
            </Autocomplete.GroupLabel>
            <Autocomplete.Collection>
              {(entry: Entry) => <Row key={entry.value} entry={entry} onRun={onRun} />}
            </Autocomplete.Collection>
          </Autocomplete.Group>
        )}
      </Autocomplete.List>
      {/* The region stands in the accessibility tree empty before use, so
          the sentence is announced when it lands. */}
      <p role="status" className={styles.empty}>
        {sections.length === 0 ? m.palette_empty({}, { locale }) : ""}
      </p>
      <p className={styles.foot} aria-hidden="true">
        <span>
          <Kbd>{ARROWS}</Kbd> {m.palette_move({}, { locale })}
        </span>
        <span>
          <Kbd>{ENTER}</Kbd> {m.palette_run({}, { locale })}
        </span>
        <span>{m.palette_every_action({}, { locale })}</span>
      </p>
    </Autocomplete.Root>
  );
}

// A nested folder by the names above it, the part the query matched in
// the accent's weight.
function MoveRow({ entry, onPick }: { entry: MoveEntry; onPick: (id: string) => void }) {
  const { target, hit } = entry;
  const name = target.name.normalize("NFC");
  return (
    <Autocomplete.Item
      value={entry}
      className={styles.item}
      onClick={() => {
        onPick(target.id);
      }}
    >
      <span className={styles.label}>
        {target.path.length > 0 && (
          <span className={styles.path}>
            {`${target.path.join(PATH_SEPARATOR)}${PATH_SEPARATOR}`}
          </span>
        )}
        {hit === null ? (
          name
        ) : (
          <>
            {name.slice(0, hit.start)}
            <span className={styles.hit}>{name.slice(hit.start, hit.end)}</span>
            {name.slice(hit.end)}
          </>
        )}
      </span>
    </Autocomplete.Item>
  );
}

interface MoveBodyProps {
  locale: Locale;
  mode: MoveMode;
  defaultQuery: string;
  inputRef: RefObject<HTMLInputElement | null>;
  onPick: (id: string) => void;
}

// The palette as the move picker: the mode chip before the input, the
// last target under Recent and the mailboxes in the tree's order; Enter
// moves to the highlighted one.
function MoveBody({ locale, mode, defaultQuery, inputRef, onPick }: MoveBodyProps) {
  const [query, setQuery] = useState(defaultQuery);
  const sections = moveSectionsOf(mode, query, locale);
  const title = m.command_thread({ action: "move" }, { locale });
  return (
    <Autocomplete.Root
      inline
      open
      items={sections}
      filter={null}
      value={query}
      onValueChange={setQuery}
      autoHighlight="always"
      locale={locale}
    >
      <div className={styles.head}>
        <span className={styles.chip} aria-hidden="true">
          <FolderInput className={styles.chipIcon} />
          {title}
        </span>
        <Autocomplete.Input
          ref={inputRef}
          className={styles.input}
          placeholder={m.move_placeholder({}, { locale })}
          aria-label={title}
        />
        <Kbd spoken={false}>{chordText(ESCAPE)}</Kbd>
      </div>
      <Autocomplete.List className={styles.list}>
        {(section: MoveSection) => (
          <Autocomplete.Group key={section.value} items={section.items}>
            <Autocomplete.GroupLabel className={styles.groupLabel}>
              {section.label}
            </Autocomplete.GroupLabel>
            <Autocomplete.Collection>
              {(entry: MoveEntry) => <MoveRow key={entry.value} entry={entry} onPick={onPick} />}
            </Autocomplete.Collection>
          </Autocomplete.Group>
        )}
      </Autocomplete.List>
      <p role="status" className={styles.empty}>
        {sections.length === 0 ? m.move_empty({}, { locale }) : ""}
      </p>
      <p className={styles.foot} aria-hidden="true">
        <span>
          <Kbd>{ARROWS}</Kbd> {m.palette_move({}, { locale })}
        </span>
        <span>
          <Kbd>{ENTER}</Kbd> {m.move_here({}, { locale })}
        </span>
      </p>
    </Autocomplete.Root>
  );
}

// Every registered command by group behind one input; in the move mode
// the mailboxes a move can go to instead. The palette closes first and
// the pick runs once the fade has ended, so it starts from the focus the
// palette gave back.
export function CommandPalette(props: CommandPaletteProps) {
  const { open, onOpenChange, onClosed, locale, commands, recent, defaultQuery = "" } = props;
  const { move, onPick } = props;
  const inputRef = useRef<HTMLInputElement>(null);
  const label =
    move === undefined
      ? m.command_palette({}, { locale })
      : m.command_thread({ action: "move" }, { locale });
  return (
    <Dialog.Root
      open={open}
      onOpenChange={onOpenChange}
      onOpenChangeComplete={(next) => {
        if (!next) {
          onClosed?.();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className={styles.backdrop} />
        <Dialog.Popup className={styles.popup} aria-label={label} initialFocus={inputRef}>
          {move === undefined ? (
            <PaletteBody
              locale={locale}
              commands={commands}
              recent={recent}
              defaultQuery={defaultQuery}
              inputRef={inputRef}
              onRun={props.onRun}
            />
          ) : (
            <MoveBody
              locale={locale}
              mode={move}
              defaultQuery={defaultQuery}
              inputRef={inputRef}
              onPick={(id) => {
                onPick?.(id);
              }}
            />
          )}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
