type Described = { message: string; requestId?: string };

// hey-api throws the parsed problem+json body, not an Error, and the daemon puts the request id
// in `instance`; an Error may carry it as `requestId` instead.
export function describeError(error: unknown): Described {
  if (typeof error !== "object" || error === null) return { message: String(error) };
  const fields = error as Record<string, unknown>;
  const text = (key: string) => (typeof fields[key] === "string" ? fields[key] : undefined);
  const message =
    text("detail") ?? text("title") ?? (error instanceof Error ? error.message : undefined);
  const requestId = text("requestId") ?? text("instance");
  return {
    message: message ?? "Something went wrong.",
    ...(requestId === undefined ? {} : { requestId }),
  };
}
