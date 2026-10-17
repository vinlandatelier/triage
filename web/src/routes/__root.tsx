import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { ThemeToggle } from "@/components/theme-toggle";
import { NotFound, RouteError } from "@/fallbacks";

export const Route = createRootRoute({
  component: RootLayout,
  notFoundComponent: NotFound,
  errorComponent: RouteError,
});

function RootLayout() {
  return (
    <>
      <header className="flex items-center justify-between border-b px-4 py-2">
        <Link to="/" className="font-semibold">
          Triage
        </Link>
        <ThemeToggle />
      </header>
      <main className="p-4">
        <Outlet />
      </main>
    </>
  );
}
