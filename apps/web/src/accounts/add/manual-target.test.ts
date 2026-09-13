// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import {
  incomingPort,
  initialManual,
  isSessionUrl,
  manualTarget,
  outgoingOf,
  usernameOf,
} from "./manual-target";
import type { ManualValues } from "./manual-target";

const ADDRESS = "sanne@dekker-mail.nl";

function filled(overrides: Partial<ManualValues> = {}): ManualValues {
  return { ...initialManual(), server: "mail.dekker-mail.nl", password: "pw", ...overrides };
}

test("the form starts with the address as username and the ports following the encryption", () => {
  const values = initialManual();
  expect(usernameOf(values, ADDRESS)).toBe(ADDRESS);
  expect(usernameOf({ ...values, username: "sanne" }, ADDRESS)).toBe("sanne");
  expect(incomingPort(values)).toBe("993");
  expect(incomingPort({ ...values, tls: "starttls" })).toBe("143");
  expect(incomingPort({ ...values, port: "1993" })).toBe("1993");
});

test("the outgoing server follows the incoming one until each part is edited", () => {
  const values = filled();
  expect(outgoingOf(values)).toEqual({ host: "mail.dekker-mail.nl", port: "465", tls: "implicit" });
  expect(outgoingOf({ ...values, tls: "starttls" })).toEqual({
    host: "mail.dekker-mail.nl",
    port: "587",
    tls: "starttls",
  });
  expect(
    outgoingOf({
      ...values,
      outgoing: { host: "smtp.dekker-mail.nl", port: null, tls: "starttls" },
    }),
  ).toEqual({ host: "smtp.dekker-mail.nl", port: "587", tls: "starttls" });
  expect(outgoingOf({ ...values, outgoing: { host: null, port: "2525", tls: null } }).port).toBe(
    "2525",
  );
});

test("a host name builds an IMAP target with both servers", () => {
  expect(manualTarget(filled({ port: " 1993 ", tls: "starttls" }), ADDRESS)).toEqual({
    host: "mail.dekker-mail.nl",
    target: {
      kind: "imap",
      username: ADDRESS,
      imap: { host: "mail.dekker-mail.nl", port: 1993, tls: "starttls" },
      smtp: { host: "mail.dekker-mail.nl", port: 587, tls: "starttls" },
    },
  });
  expect(manualTarget(filled({ username: "sanne" }), ADDRESS)?.target).toMatchObject({
    username: "sanne",
  });
});

test("a session URL builds a JMAP target on its host", () => {
  expect(isSessionUrl("HTTPS://localhost:8443/jmap")).toBe(true);
  expect(isSessionUrl("mail.dekker-mail.nl")).toBe(false);
  expect(manualTarget(filled({ server: " https://localhost:8443/jmap " }), ADDRESS)).toEqual({
    host: "localhost",
    target: { kind: "jmap", sessionUrl: "https://localhost:8443/jmap" },
  });
});

test.each([
  ["http://mail.dekker-mail.nl/jmap"],
  ["https://sanne:pw@mail.dekker-mail.nl/jmap"],
  ["https://127.0.0.1/jmap"],
  ["https://"],
  ["mail.dekker-mail.nl:993"],
  ["127.0.0.1"],
  ["mail dekker-mail.nl"],
])("%s is refused before anything is sent", (server) => {
  expect(manualTarget(filled({ server }), ADDRESS)).toBeNull();
});

test("a port outside sixteen bits or an outgoing literal is refused", () => {
  expect(manualTarget(filled({ port: "70000" }), ADDRESS)).toBeNull();
  expect(manualTarget(filled({ port: "abc" }), ADDRESS)).toBeNull();
  expect(
    manualTarget(filled({ outgoing: { host: "10.0.0.1", port: null, tls: null } }), ADDRESS),
  ).toBeNull();
  expect(
    manualTarget(filled({ outgoing: { host: null, port: "0", tls: null } }), ADDRESS),
  ).toBeNull();
});
