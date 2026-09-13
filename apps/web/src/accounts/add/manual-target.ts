// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fitsHostName } from "@huliho/core";
import type { AccountTarget, TlsMode } from "@huliho/core";

import { PORT_MAX, PORT_MIN, defaultPort } from "./presets";

export interface OutgoingValues {
  // Null follows the incoming server.
  host: string | null;
  port: string | null;
  tls: TlsMode | null;
}

export interface ManualValues {
  server: string;
  // Null follows the encryption's default port.
  port: string | null;
  tls: TlsMode;
  // Null follows the address typed on the first screen.
  username: string | null;
  password: string;
  outgoing: OutgoingValues;
}

export interface OutgoingServer {
  host: string;
  port: string;
  tls: TlsMode;
}

export interface BuiltTarget {
  target: AccountTarget;
  // What the connecting sentence names.
  host: string;
}

export function initialManual(): ManualValues {
  return {
    server: "",
    port: null,
    tls: "implicit",
    username: null,
    password: "",
    outgoing: { host: null, port: null, tls: null },
  };
}

export function usernameOf(values: ManualValues, address: string): string {
  return values.username ?? address;
}

// A Server value that is a JMAP session URL rather than a host name.
export function isSessionUrl(server: string): boolean {
  return server.trim().toLowerCase().startsWith("https://");
}

export function incomingPort(values: ManualValues): string {
  return values.port ?? String(defaultPort("imap", values.tls));
}

// The outgoing server as it stands: each part follows the incoming one
// until edited.
export function outgoingOf(values: ManualValues): OutgoingServer {
  const tls = values.outgoing.tls ?? values.tls;
  return {
    host: values.outgoing.host ?? values.server,
    port: values.outgoing.port ?? String(defaultPort("smtp", tls)),
    tls,
  };
}

function portOf(text: string): number | null {
  const port = Number(text.trim());
  return Number.isInteger(port) && port >= PORT_MIN && port <= PORT_MAX ? port : null;
}

// A session URL on a named host without userinfo, as the server accepts it.
function sessionTarget(server: string): BuiltTarget | null {
  let url: URL;
  try {
    url = new URL(server);
  } catch {
    return null;
  }
  if (url.username !== "" || url.password !== "" || !fitsHostName(url.hostname)) {
    return null;
  }
  return { host: url.hostname, target: { kind: "jmap", sessionUrl: url.toString() } };
}

// The target the form describes; null when the server would refuse it.
export function manualTarget(values: ManualValues, address: string): BuiltTarget | null {
  const server = values.server.trim();
  if (isSessionUrl(server)) {
    return sessionTarget(server);
  }
  const outgoing = outgoingOf(values);
  const outgoingHost = outgoing.host.trim();
  const port = portOf(incomingPort(values));
  const outgoingPort = portOf(outgoing.port);
  if (!fitsHostName(server) || !fitsHostName(outgoingHost) || port === null) {
    return null;
  }
  if (outgoingPort === null) {
    return null;
  }
  return {
    host: server,
    target: {
      kind: "imap",
      username: usernameOf(values, address),
      imap: { host: server, port, tls: values.tls },
      smtp: { host: outgoingHost, port: outgoingPort, tls: outgoing.tls },
    },
  };
}
