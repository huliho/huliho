// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { UserRow } from "@huliho/core";
import { grantableRoles } from "@huliho/core";
import { relativeTime } from "@huliho/i18n";

import { Button } from "../../design-system/button";
import rowList from "../../design-system/row-list.module.css";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { useSidebarShown } from "../layout";
import { RoleBadge } from "./role-badge";
import styles from "./user-table.module.css";

export interface UserTableProps {
  rows: UserRow[];
  locale: Locale;
  now: Date;
  // The signed-in admin: their own row offers no reset, nor does a row above their role.
  actor: Pick<UserRow, "id" | "role">;
  onReset: (user: UserRow) => void;
}

interface RowProps {
  row: UserRow;
  locale: Locale;
  now: Date;
  own: boolean;
  // Whether the actor may reset this row, as the server decides it.
  reachable: boolean;
  onReset: (user: UserRow) => void;
}

function lastActive(row: UserRow, locale: Locale, now: Date): string {
  if (row.lastActiveAt === null) {
    return m.users_never_active({}, { locale });
  }
  return relativeTime(locale, new Date(row.lastActiveAt), now) ?? m.active_now({}, { locale });
}

// The own row says so instead of offering a reset: nobody resets themselves.
// A row above the actor's role offers nothing, since the server would refuse.
function RowAction({ row, locale, own, reachable, onReset }: Omit<RowProps, "now">) {
  if (own) {
    return <span className={styles.you}>{m.users_you({}, { locale })}</span>;
  }
  if (!reachable) {
    return null;
  }
  return (
    <Button
      aria-label={m.users_reset_for({ name: row.name }, { locale })}
      onClick={() => {
        onReset(row);
      }}
    >
      {m.users_reset({}, { locale })}
    </Button>
  );
}

function TableRow({ now, ...rest }: RowProps) {
  return (
    <tr>
      <td className={styles.name}>{rest.row.name}</td>
      <td>{rest.row.login}</td>
      <td>
        <RoleBadge role={rest.row.role} locale={rest.locale} />
      </td>
      <td className={styles.nowrap}>{lastActive(rest.row, rest.locale, now)}</td>
      <td className={styles.action}>
        <RowAction {...rest} />
      </td>
    </tr>
  );
}

function CardRow({ now, ...rest }: RowProps) {
  return (
    <li className={rowList.row}>
      <div className={rowList.facts}>
        <span className={styles.name}>{rest.row.name}</span>
        <span className={styles.login}>{rest.row.login}</span>
        <span className={rowList.meta}>
          <RoleBadge role={rest.row.role} locale={rest.locale} />
          <span>{lastActive(rest.row, rest.locale, now)}</span>
        </span>
      </div>
      <RowAction {...rest} />
    </li>
  );
}

function HeaderRow({ locale }: { locale: Locale }) {
  return (
    <tr>
      <th scope="col">{m.users_name({}, { locale })}</th>
      <th scope="col">{m.users_login({}, { locale })}</th>
      <th scope="col">{m.users_role({}, { locale })}</th>
      <th scope="col">{m.users_last_active({}, { locale })}</th>
      <th scope="col" aria-label={m.users_actions({}, { locale })} />
    </tr>
  );
}

// A wide screen gets the table; a phone gets one card per user, since
// four columns and a button never fit its width.
export function UserTable({ rows, locale, now, actor, onReset }: UserTableProps) {
  const wide = useSidebarShown();
  const reachableRoles = grantableRoles(actor.role);
  const propsOf = (row: UserRow): RowProps => ({
    row,
    locale,
    now,
    own: row.id === actor.id,
    reachable: reachableRoles.includes(row.role),
    onReset,
  });
  if (!wide) {
    return (
      <ul className={rowList.list} aria-label={m.users_heading({}, { locale })}>
        {rows.map((row) => (
          <CardRow key={row.id} {...propsOf(row)} />
        ))}
      </ul>
    );
  }
  return (
    <div className={styles.scroll}>
      <table className={styles.table}>
        <thead>
          <HeaderRow locale={locale} />
        </thead>
        <tbody>
          {rows.map((row) => (
            <TableRow key={row.id} {...propsOf(row)} />
          ))}
        </tbody>
      </table>
    </div>
  );
}
