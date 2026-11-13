// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { IncomingMessage, ServerResponse } from "node:http";
import { json } from "node:stream/consumers";

import type { Plugin } from "vite";

import {
  ACCOUNT_LIST_BODY,
  PLAIN_SHAPE,
  SESSION_BODY,
  SIGNED_OUT_BODY,
  jmapSessionBody,
} from "./api-bodies";
import { answerCalls, methodCallsOf, serverFor } from "./mail-answers";
import { corpusFor } from "./mail-corpus";
import { MAILBOXES, listedMailboxes } from "./mail-mocks";

// The server's session cookie, which lighthouse-session.cjs sets before
// the mail route's audit; every request without it reads as signed out.
const SESSION_COOKIE = "huliho_session=";
const OK = 200;
const UNAUTHORIZED = 401;
const NOT_FOUND = 404;
const SERVER_ERROR = 500;
const JMAP_SESSION = /^\/jmap\/([^/]+)\/session$/;
const JMAP_API = /^\/jmap\/[^/]+$/;

// What a signed-in GET answers, by its path under /api.
const READS = new Map<string, unknown>([
  ["/session", SESSION_BODY],
  ["/accounts", ACCOUNT_LIST_BODY],
  ["/preferences", {}],
  ["/sender-policies", []],
]);

function send(res: ServerResponse, status: number, body: unknown): void {
  res.statusCode = status;
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
}

function signedIn(req: IncomingMessage): boolean {
  return (req.headers.cookie ?? "").split("; ").some((pair) => pair.startsWith(SESSION_COOKIE));
}

// The API of the server for one owner with one account and the e2e
// corpus behind it, so the preview loads the sign-in screen and the mail
// route as the binary serves them.
export function previewApi(): Plugin {
  const mail = serverFor(corpusFor(MAILBOXES), listedMailboxes(MAILBOXES, false));
  const answer = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
    if (!signedIn(req)) {
      send(res, UNAUTHORIZED, SIGNED_OUT_BODY);
      return;
    }
    const path = new URL(req.url ?? "/", "http://preview").pathname;
    const read = req.method === "GET" ? READS.get(path) : undefined;
    const accountId = req.method === "GET" ? JMAP_SESSION.exec(path)?.[1] : undefined;
    if (read !== undefined) {
      send(res, OK, read);
    } else if (accountId !== undefined) {
      send(res, OK, jmapSessionBody(accountId, PLAIN_SHAPE));
    } else if (req.method === "POST" && JMAP_API.test(path)) {
      const calls = methodCallsOf(await json(req));
      send(res, OK, { methodResponses: answerCalls(mail, calls), sessionState: "s1" });
    } else {
      send(res, NOT_FOUND, { error: "not_found" });
    }
  };
  return {
    name: "huliho-preview-api",
    configurePreviewServer(server) {
      server.middlewares.use("/api", (req, res) => {
        answer(req, res).catch((error: unknown) => {
          server.config.logger.error(`${req.method ?? ""} /api${req.url ?? ""}: ${String(error)}`);
          send(res, SERVER_ERROR, { error: "internal" });
        });
      });
    },
  };
}
