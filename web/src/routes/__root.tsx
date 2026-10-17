import type { ErrorComponentProps } from "@tanstack/react-router";
import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { describeError } from "../error";

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

function NotFound() {
  return (
    <section>
      <h1>Not found</h1>
      <p>Nothing lives at this address.</p>
      <Link to="/">Go home</Link>
    </section>
  );
}

function RouteError({ error }: ErrorComponentProps) {
  const { message, requestId } = describeError(error);
  return (
    <section role="alert">
      <h1>Something went wrong</h1>
      <p>{message}</p>
      {requestId && (
        <p>
          Request id: <code>{requestId}</code>
        </p>
      )}
    </section>
  );
}
