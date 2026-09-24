// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  lazyRouteComponent,
  redirect,
} from "@tanstack/react-router";

import { mayManageUsers } from "@huliho/core";
import type { AccountRow, SessionInfo } from "@huliho/core";
import { accountsQueryOptions, sessionQueryOptions } from "@huliho/state";
import { InboxRedirect } from "./mail/inbox-redirect";
import { landingAccount } from "./mail/last-account";
import { MailShell } from "./mail/mail-shell";
import { MailboxPane } from "./mail/mailbox-pane";
import { RootLayout } from "./shell/root-layout";
import { RouteError, RoutePending } from "./shell/route-fallbacks";
import { SignedInLayout } from "./shell/signed-in-layout";
import { ChoosePassword } from "./sign-in/choose-password";
import { SignIn } from "./sign-in/sign-in";

interface RouterContext {
  queryClient: QueryClient;
}

type Home = "/" | "/choose-password";

export const queryClient = new QueryClient();

// The settings screens arrive as one chunk on the first settings visit;
// the add-account card as another on the first visit to it.
const settings = () => import("./settings/pages");
const addAccount = () => import("./accounts/add/add-account");

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});

// Where a session belongs: the shell or the forced password step, until
// the one-time password is replaced. Without one, sign-in.
function homeOf(session: SessionInfo | null): Home | "/sign-in" {
  if (session === null) {
    return "/sign-in";
  }
  return session.passwordChangeRequired ? "/choose-password" : "/";
}

// A guarded route names its home; a session that belongs elsewhere goes there.
async function requireHome(context: RouterContext, home: Home): Promise<void> {
  const actual = homeOf(await context.queryClient.query(sessionQueryOptions));
  if (actual !== home) {
    redirect({ to: actual, throw: true });
  }
}

// Below the admin role the users page does not exist; the sessions page
// is the nearest one that does.
async function requireAdmin(context: RouterContext): Promise<void> {
  const session = await context.queryClient.query(sessionQueryOptions);
  if (session === null || !mayManageUsers(session.user.role)) {
    redirect({ to: "/settings/sessions", throw: true });
  }
}

interface AddAccountSearch {
  // The row to reconnect; absent for a fresh add.
  reconnect?: string;
}

// The shell has nothing to show without an account, so a session with
// none starts by adding one.
async function requireAccounts(context: RouterContext): Promise<AccountRow[]> {
  const list = await context.queryClient.query(accountsQueryOptions);
  if (list.accounts.length === 0) {
    redirect({ to: "/accounts/new", throw: true });
  }
  return list.accounts;
}

// The row a reconnect opens on; a row that is gone opens the card plain.
async function reconnectRow(
  context: RouterContext,
  id: string | undefined,
): Promise<AccountRow | null> {
  if (id === undefined) {
    return null;
  }
  const list = await context.queryClient.query(accountsQueryOptions);
  return list.accounts.find((account) => account.id === id) ?? null;
}

// Every route behind a session guard renders inside this layout.
const signedInRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "signed-in",
  component: SignedInLayout,
});

// The root lands in an account: the one this device used last, else the oldest.
const homeRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/",
  beforeLoad: async ({ context }) => {
    await requireHome(context, "/");
    const accountId = landingAccount(await requireAccounts(context));
    if (accountId !== null) {
      redirect({ to: "/mail/$accountId", params: { accountId }, throw: true });
    }
  },
});

// An account the session does not hold sends the visit back to the root.
const mailRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/mail/$accountId",
  component: MailShell,
  beforeLoad: async ({ context, params }) => {
    await requireHome(context, "/");
    const accounts = await requireAccounts(context);
    if (!accounts.some((account) => account.id === params.accountId)) {
      redirect({ to: "/", throw: true });
    }
  },
});

const mailIndexRoute = createRoute({
  getParentRoute: () => mailRoute,
  path: "/",
  component: InboxRedirect,
});

const mailboxRoute = createRoute({
  getParentRoute: () => mailRoute,
  path: "/$mailboxId",
  component: MailboxPane,
});

// The thread open in the mailbox; the shell draws it beside, below or
// over the list, so the route itself renders nothing of its own.
const threadRoute = createRoute({
  getParentRoute: () => mailboxRoute,
  path: "/$threadId",
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  component: SignIn,
  beforeLoad: async ({ context }) => {
    // An unreachable API reads as signed out, so the form still renders.
    const session = await context.queryClient.query(sessionQueryOptions).catch(() => null);
    if (session !== null) {
      redirect({ to: homeOf(session), throw: true });
    }
  },
});

const choosePasswordRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/choose-password",
  component: ChoosePassword,
  beforeLoad: ({ context }) => requireHome(context, "/choose-password"),
});

const addAccountRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/accounts/new",
  component: lazyRouteComponent(addAccount, "AddAccount"),
  validateSearch: (search: Record<string, unknown>): AddAccountSearch =>
    typeof search["reconnect"] === "string" ? { reconnect: search["reconnect"] } : {},
  beforeLoad: ({ context }) => requireHome(context, "/"),
  loaderDeps: ({ search }) => ({ reconnect: search.reconnect }),
  loader: ({ context, deps }) => reconnectRow(context, deps.reconnect),
});

const settingsRoute = createRoute({
  getParentRoute: () => signedInRoute,
  path: "/settings",
  component: lazyRouteComponent(settings, "SettingsPage"),
  beforeLoad: ({ context }) => requireHome(context, "/"),
});

const settingsIndexRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/",
  component: lazyRouteComponent(settings, "SettingsIndex"),
});

const accountsRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/accounts",
  component: lazyRouteComponent(settings, "AccountsPage"),
});

const appearanceRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/appearance",
  component: lazyRouteComponent(settings, "AppearancePage"),
});

const sessionsRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/sessions",
  component: lazyRouteComponent(settings, "SessionsPage"),
});

const aboutRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/about",
  component: lazyRouteComponent(settings, "AboutSettings"),
});

const usersRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/users",
  component: lazyRouteComponent(settings, "UsersPage"),
  beforeLoad: ({ context }) => requireAdmin(context),
});

const routeTree = rootRoute.addChildren([
  signInRoute,
  signedInRoute.addChildren([
    homeRoute,
    mailRoute.addChildren([mailIndexRoute, mailboxRoute.addChildren([threadRoute])]),
    choosePasswordRoute,
    addAccountRoute,
    settingsRoute.addChildren([
      settingsIndexRoute,
      accountsRoute,
      appearanceRoute,
      sessionsRoute,
      aboutRoute,
      usersRoute,
    ]),
  ]),
]);

export const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPendingComponent: RoutePending,
  defaultErrorComponent: RouteError,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
