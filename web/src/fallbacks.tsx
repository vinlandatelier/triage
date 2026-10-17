import type { ErrorComponentProps } from "@tanstack/react-router";
import { Link, useRouter } from "@tanstack/react-router";
import { describeError } from "@/error";

export function NotFound() {
  return (
    <section>
      <h1>Not found</h1>
      <p>Nothing lives at this address.</p>
      <Link to="/">Go home</Link>
    </section>
  );
}

export function RouteError({ error, reset }: ErrorComponentProps) {
  const router = useRouter();
  const { message, requestId } = describeError(error);
  // reset() alone re-renders the failed match, which throws again after a loader error.
  const retry = () => {
    reset();
    void router.invalidate();
  };
  return (
    <section role="alert" className="mx-auto flex max-w-md flex-col gap-2">
      <h1 className="text-xl font-semibold">Something went wrong</h1>
      <p className="text-muted-foreground">{message}</p>
      {requestId && (
        <p className="text-sm text-muted-foreground">
          Request id: <code>{requestId}</code>
        </p>
      )}
      <button type="button" onClick={retry}>
        Try again
      </button>
      <Link to="/">Go home</Link>
    </section>
  );
}
