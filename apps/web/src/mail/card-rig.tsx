// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, MailCache, Preferences, SenderPolicy } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, vi } from "vitest";

import { ToastProvider, Toasts } from "../design-system/toast";
import { forgetLinkKey, linkKey } from "../open/link-key";
import { stubWidthQueries } from "../shell/width-queries-rig";
import { FASTMAIL, FIXED_NOW, THREAD, THREAD_ID, fixtureCache } from "./fixtures";
import type { BodyAnswer } from "./fixtures";
import { MessageCard } from "./message-card";
import { startOfDay } from "./row-time";
import type { PlannedMessage } from "./thread-messages";

// The rig the card tests render in: one card in a list, a fixture cache
// with the bodies of the test, the sender policies the stubbed server
// answers and the queries a card reads. The newest message of the
// fixture thread is the card's message unless a test says otherwise.
export const NEWEST_ID = "e-3";
const TODAY = startOfDay(FIXED_NOW);

export const SENDER_POLICIES_ROUTE = "/api/sender-policies";

// One request the stubbed server saw.
export interface Seen {
  method: string;
  url: string;
  body: unknown;
}

export interface Answers {
  // The policies the server lists, or the status of its refusal.
  policies?: SenderPolicy[] | number;
  // The status a write gets.
  writeStatus?: number;
}

// jsdom has no ResizeObserver; the frame watches its document with one.
class StillObserver {
  observe(): void {
    return undefined;
  }

  disconnect(): void {
    return undefined;
  }
}

export function newest(): EmailHeader {
  const header = new Map(Object.entries(THREAD.emails)).get(NEWEST_ID);
  if (header === undefined) {
    throw new Error("the fixture thread has no newest message");
  }
  return header;
}

// The newest message of the fixture thread as unread.
export function unread(): EmailHeader {
  return { ...newest(), keywords: {} };
}

export function planned(email: EmailHeader): PlannedMessage {
  return { email, unread: !("$seen" in email.keywords), expanded: true, older: false };
}

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  return input instanceof URL ? input.href : input.url;
}

// What the server answers a request on the policies route.
function answerOf(method: string, answers: Answers): Response {
  if (method !== "GET") {
    return new Response(null, { status: answers.writeStatus ?? 204 });
  }
  const policies = answers.policies ?? [];
  return typeof policies === "number"
    ? new Response(null, { status: policies })
    : Response.json(policies);
}

// The server behind the card: the policies list and every write recorded.
export function stubServer(answers: Answers = {}): Seen[] {
  const seen: Seen[] = [];
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) => {
    const url = urlOf(input);
    const method = init?.method ?? "GET";
    const body: unknown = typeof init?.body === "string" ? JSON.parse(init.body) : null;
    seen.push({ method, url, body });
    const known = url.startsWith(SENDER_POLICIES_ROUTE);
    return Promise.resolve(known ? answerOf(method, answers) : new Response(null, { status: 404 }));
  });
  return seen;
}

interface Options {
  message?: PlannedMessage;
  bodies?: Record<string, BodyAnswer>;
  expanded?: boolean;
  preferences?: Preferences;
}

export interface Rendered {
  cache: ReturnType<typeof fixtureCache>;
}

interface HarnessProps {
  message: PlannedMessage;
  expanded: boolean;
  cache: MailCache;
}

// One card in a list; its fold button opens and folds it.
function Harness({ message, expanded, cache }: HarnessProps) {
  const [shown, setShown] = useState(expanded);
  return (
    <ToastProvider>
      <ol role="list">
        <MessageCard
          locale="en"
          today={TODAY}
          message={message}
          expanded={shown}
          cache={cache}
          accountId={FASTMAIL.id}
          onToggle={() => {
            setShown(!shown);
          }}
        />
      </ol>
      <Toasts />
    </ToastProvider>
  );
}

export function renderCard(options: Options = {}): Rendered {
  const message = options.message ?? planned(newest());
  const cache = fixtureCache({}, { [THREAD_ID]: THREAD }, options.bodies ?? {});
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.preferences, options.preferences ?? {});
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => (
        <Harness message={message} expanded={options.expanded ?? true} cache={cache} />
      ),
    }),
    history: createMemoryHistory({ initialEntries: ["/mail/acc-1/mb-inbox"] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { cache };
}

// The frame once it rendered; the test environment loads no srcdoc, so
// the load event is sent by hand.
export async function frame(): Promise<HTMLIFrameElement> {
  const found = await screen.findByTitle(/^Message from/);
  if (!(found instanceof HTMLIFrameElement)) {
    throw new TypeError("the card renders no frame");
  }
  fireEvent.load(found);
  return found;
}

// The media queries answer for the desktop layout; the theme follows
// the document attribute the test sets, else the light scheme. The
// device holds its link key, as it does once the mail route loaded.
export function mockCardBox(): void {
  beforeEach(() => {
    stubWidthQueries({ wide: true });
    vi.stubGlobal("ResizeObserver", StillObserver);
    linkKey();
  });
  afterEach(() => {
    forgetLinkKey();
    document.documentElement.removeAttribute("data-theme");
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
}
