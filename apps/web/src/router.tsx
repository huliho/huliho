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

import type { SessionInfo } from "@huliho/core";
import { sessionQueryOptions } from "@huliho/state";
import { App } from "./app";
import { AboutSettings } from "./settings/about";
import { SessionsPage } from "./settings/sessions/sessions-page";
import { SettingsIndex } from "./settings/settings-index";
import { SettingsPage } from "./settings/settings-page";
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

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: App,
  beforeLoad: ({ context }) => requireHome(context, "/"),
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

const routeTree = rootRoute.addChildren([
  shellRoute,
  signInRoute,
  choosePasswordRoute,
  settingsRoute.addChildren([settingsIndexRoute, sessionsRoute, aboutRoute]),
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
