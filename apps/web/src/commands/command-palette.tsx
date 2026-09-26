// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import { Dialog } from "@base-ui/react/dialog";
import { Search } from "lucide-react";
import { useRef, useState } from "react";
import type { RefObject } from "react";

import { Kbd } from "../design-system/kbd";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { KeyCaps } from "./key-caps";
import { ESCAPE, chordText } from "./keys";
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

// Every registered command by group behind one input. The palette
// closes first and the command runs once the fade has ended, so it
// starts from the focus the palette gave back.
export function CommandPalette(props: CommandPaletteProps) {
  const { open, onOpenChange, onClosed, locale, commands, recent, defaultQuery = "" } = props;
  const inputRef = useRef<HTMLInputElement>(null);
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
        <Dialog.Popup
          className={styles.popup}
          aria-label={m.command_palette({}, { locale })}
          initialFocus={inputRef}
        >
          <PaletteBody
            locale={locale}
            commands={commands}
            recent={recent}
            defaultQuery={defaultQuery}
            inputRef={inputRef}
            onRun={props.onRun}
          />
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
