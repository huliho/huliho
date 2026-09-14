// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  redirect,
} from "@tanstack/react-router";

import { mayManageUsers } from "@huliho/core";
import type { AccountRow, SessionInfo } from "@huliho/core";
import { accountsQueryOptions, sessionQueryOptions } from "@huliho/state";
import { AddAccount } from "./accounts/add/add-account";
import { App } from "./app";
import { AboutSettings } from "./settings/about";
import { SessionsPage } from "./settings/sessions/sessions-page";
import { SettingsIndex } from "./settings/settings-index";
import { SettingsPage } from "./settings/settings-page";
import { UsersPage } from "./settings/users/users-page";
import { RootLayout } from "./shell/root-layout";
import { RouteError, RoutePending } from "./shell/route-fallbacks";
import { ChoosePassword } from "./sign-in/choose-password";
import { SignIn } from "./sign-in/sign-in";

interface RouterContext {
  queryClient: QueryClient;
}

type Home = "/" | "/choose-password";

export const queryClient = new QueryClient();

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
async function requireAccount(context: RouterContext): Promise<void> {
  const list = await context.queryClient.query(accountsQueryOptions);
  if (list.accounts.length === 0) {
    redirect({ to: "/accounts/new", throw: true });
  }
}

// The row a reconnect opens on; a row that signs in through a consent
// or one that is gone opens the card plain.
async function reconnectRow(
  context: RouterContext,
  id: string | undefined,
): Promise<AccountRow | null> {
  if (id === undefined) {
    return null;
  }
  const list = await context.queryClient.query(accountsQueryOptions);
  const row = list.accounts.find((account) => account.id === id);
  return row !== undefined && row.authMethod !== "oauth2" ? row : null;
}

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: App,
  beforeLoad: async ({ context }) => {
    await requireHome(context, "/");
    await requireAccount(context);
  },
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
  getParentRoute: () => rootRoute,
  path: "/choose-password",
  component: ChoosePassword,
  beforeLoad: ({ context }) => requireHome(context, "/choose-password"),
});

const addAccountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts/new",
  component: AddAccount,
  validateSearch: (search: Record<string, unknown>): AddAccountSearch =>
    typeof search["reconnect"] === "string" ? { reconnect: search["reconnect"] } : {},
  beforeLoad: ({ context }) => requireHome(context, "/"),
  loaderDeps: ({ search }) => ({ reconnect: search.reconnect }),
  loader: ({ context, deps }) => reconnectRow(context, deps.reconnect),
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
  beforeLoad: ({ context }) => requireHome(context, "/"),
});

const settingsIndexRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/",
  component: SettingsIndex,
});

const sessionsRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/sessions",
  component: SessionsPage,
});

const aboutRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/about",
  component: AboutSettings,
});

const usersRoute = createRoute({
  getParentRoute: () => settingsRoute,
  path: "/users",
  component: UsersPage,
  beforeLoad: ({ context }) => requireAdmin(context),
});

const routeTree = rootRoute.addChildren([
  shellRoute,
  signInRoute,
  choosePasswordRoute,
  addAccountRoute,
  settingsRoute.addChildren([settingsIndexRoute, sessionsRoute, aboutRoute, usersRoute]),
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
