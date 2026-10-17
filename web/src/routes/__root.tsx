import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { NotFound, RouteError } from "../fallbacks";

export const Route = createRootRoute({
  component: RootLayout,
  notFoundComponent: NotFound,
  errorComponent: RouteError,
});

function RootLayout() {
  return (
    <>
      <header>
        <Link to="/">Triage</Link>
      </header>
      <main>
        <Outlet />
      </main>
    </>
  );
}
