import type { QueryClient } from "@tanstack/react-query";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createRootRouteWithContext, Link, Outlet } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { ThemeToggle } from "@/components/theme-toggle";
import { NotFound, RouteError } from "@/fallbacks";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  component: RootLayout,
  notFoundComponent: NotFound,
  errorComponent: RootError,
});

function RootLayout() {
  return (
    <Shell>
      <Outlet />
    </Shell>
  );
}

// The root errorComponent replaces RootLayout, so it wraps itself in the same shell.
function RootError(props: ErrorComponentProps) {
  return (
    <Shell>
      <RouteError {...props} />
    </Shell>
  );
}

function Shell({ children }: { children: ReactNode }) {
  return (
    <>
      <header className="flex items-center justify-between border-b px-4 py-2">
        <Link to="/" className="font-semibold">
          Triage
        </Link>
        <ThemeToggle />
      </header>
      <main className="p-4">{children}</main>
    </>
  );
}
