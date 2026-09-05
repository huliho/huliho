// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SessionRow } from "@huliho/core";
import { relativeTime } from "@huliho/i18n";
import { useEffect, useRef } from "react";

import { Badge } from "../../design-system/badge";
import { Button } from "../../design-system/button";
import rowList from "../../design-system/row-list.module.css";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { deviceLabel, isUnknownDevice } from "./device-label";
import styles from "./session-list.module.css";

interface SessionListProps {
  rows: SessionRow[];
  locale: Locale;
  now: Date;
  onRevoke: (id: string) => void;
  onRevokeOthers: () => void;
}

interface SessionItemProps {
  row: SessionRow;
  locale: Locale;
  now: Date;
  onRevoke: (id: string) => void;
}

function deviceTitle(row: SessionRow, locale: Locale): string {
  const device = deviceLabel(row.device, locale);
  if (!row.current) {
    return device;
  }
  return isUnknownDevice(row.device)
    ? m.sessions_this_device_only({}, { locale })
    : m.sessions_this_device({ device }, { locale });
}

function SessionItem({ row, locale, now, onRevoke }: SessionItemProps) {
  const seen = relativeTime(locale, new Date(row.lastSeenAt), now) ?? m.active_now({}, { locale });
  return (
    <li className={rowList.row}>
      <div className={rowList.facts}>
        <span className={styles.device}>{deviceTitle(row, locale)}</span>
        <span className={rowList.meta}>
          {row.address !== null && (
            <>
              <span className={styles.address}>{row.address}</span>
              <span aria-hidden="true">·</span>
            </>
          )}
          <span>{seen}</span>
        </span>
      </div>
      {row.current ? (
        <Badge tone="accent">{m.sessions_current({}, { locale })}</Badge>
      ) : (
        <Button
          aria-label={m.sessions_revoke_device(
            { device: deviceLabel(row.device, locale) },
            { locale },
          )}
          data-session={row.id}
          onClick={() => {
            onRevoke(row.id);
          }}
        >
          {m.sessions_revoke({}, { locale })}
        </Button>
      )}
    </li>
  );
}

export function SessionList({ rows, locale, now, onRevoke, onRevokeOthers }: SessionListProps) {
  const list = useRef<HTMLUListElement>(null);
  // Rows revoked but still rendered until the cache update lands; a quick
  // second keypress must not hand focus to one of them.
  const leaving = useRef(new Set<string>());
  useEffect(() => {
    for (const id of leaving.current) {
      if (!rows.some((row) => row.id === id)) {
        leaving.current.delete(id);
      }
    }
  }, [rows]);
  const others = rows.filter((row) => !row.current).length;
  // The pressed button leaves with its row, so focus moves on before it does:
  // to the next row's button, else the previous one's, else the list itself.
  const revokeAndKeepFocus = (id: string): void => {
    leaving.current.add(id);
    const index = rows.findIndex((row) => row.id === id);
    const staying = (row: SessionRow): boolean => !row.current && !leaving.current.has(row.id);
    const neighbor = rows.slice(index + 1).find(staying) ?? rows.slice(0, index).findLast(staying);
    const target =
      neighbor === undefined
        ? list.current
        : list.current?.querySelector<HTMLElement>(`[data-session="${CSS.escape(neighbor.id)}"]`);
    target?.focus();
    onRevoke(id);
  };
  const revokeOthersAndKeepFocus = (): void => {
    list.current?.focus();
    onRevokeOthers();
  };
  return (
    <>
      <ul
        ref={list}
        className={rowList.list}
        tabIndex={-1}
        aria-label={m.sessions_heading({}, { locale })}
      >
        {rows.map((row) => (
          <SessionItem
            key={row.id}
            row={row}
            locale={locale}
            now={now}
            onRevoke={revokeAndKeepFocus}
          />
        ))}
      </ul>
      {others > 0 && (
        <div className={styles.footer}>
          <p className={styles.note}>{m.sessions_revoke_others_note({}, { locale })}</p>
          <Button variant="danger" onClick={revokeOthersAndKeepFocus}>
            {m.sessions_revoke_others({}, { locale })}
          </Button>
        </div>
      )}
    </>
  );
}
