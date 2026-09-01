// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient } from "@tanstack/react-query";
import {
  Outlet,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  redirect,
} from "@tanstack/react-router";

import { sessionQueryOptions } from "@huliho/state";
import { App } from "./app";
import { AboutSettings } from "./settings/about";
import { RouteError, RoutePending } from "./shell/route-fallbacks";
import { SignIn } from "./sign-in/sign-in";

interface RouterContext {
  queryClient: QueryClient;
}

export const queryClient = new QueryClient();

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
});

async function requireSession(context: RouterContext): Promise<void> {
  const session = await context.queryClient.query(sessionQueryOptions);
  if (session === null) {
    redirect({ to: "/sign-in", throw: true });
  }
}

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: App,
  beforeLoad: ({ context }) => requireSession(context),
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  component: SignIn,
  beforeLoad: async ({ context }) => {
    // An unreachable API reads as signed out, so the form still renders.
    const session = await context.queryClient.query(sessionQueryOptions).catch(() => null);
    if (session !== null) {
      redirect({ to: "/", throw: true });
    }
  },
});

const aboutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/about",
  component: AboutSettings,
  beforeLoad: ({ context }) => requireSession(context),
});

const routeTree = rootRoute.addChildren([shellRoute, signInRoute, aboutRoute]);

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
