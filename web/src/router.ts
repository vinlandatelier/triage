import type { QueryClient } from "@tanstack/react-query";
import { createRouter, type RouterHistory } from "@tanstack/react-router";
import { NotFound, RouteError } from "@/fallbacks";
import { routeTree } from "@/routeTree.gen";

export function createAppRouter({
  queryClient,
  ...options
}: {
  queryClient: QueryClient;
  history?: RouterHistory;
}) {
  // Query owns caching, so a preload always runs the loader and lets Query decide what is fresh.
  // Without the default components, a child route error or notFound() replaces the root shell.
  return createRouter({
    routeTree,
    ...options,
    context: { queryClient },
    defaultPreloadStaleTime: 0,
    defaultErrorComponent: RouteError,
    defaultNotFoundComponent: NotFound,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
