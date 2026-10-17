import { createRouter, type RouterHistory } from "@tanstack/react-router";
import { NotFound, RouteError } from "@/fallbacks";
import { routeTree } from "@/routeTree.gen";

export function createAppRouter(options: { history?: RouterHistory } = {}) {
  return createRouter({
    routeTree,
    ...options,
    // Without defaults, a child route error or notFound() bubbles to the root boundary and replaces the header.
    defaultErrorComponent: RouteError,
    defaultNotFoundComponent: NotFound,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
