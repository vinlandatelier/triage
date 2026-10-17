type Described = { message: string; requestId?: string };

const FALLBACK = "Something went wrong.";

// hey-api throws the parsed problem+json body, not an Error, and the daemon puts the request id
// in `instance`; an Error may carry it as `requestId` instead.
export function describeError(error: unknown): Described {
  if (error === null || error === undefined) return { message: FALLBACK };
  if (typeof error !== "object") return { message: String(error) || FALLBACK };
  const fields = error as Record<string, unknown>;
  const text = (key: string) => {
    const value = fields[key];
    return typeof value === "string" && value !== "" ? value : undefined;
  };
  const message = text("detail") ?? text("title") ?? text("message");
  const requestId = text("requestId") ?? text("instance");
  return {
    message: message ?? FALLBACK,
    ...(requestId === undefined ? {} : { requestId }),
  };
}
