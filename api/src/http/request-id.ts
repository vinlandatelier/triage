import type { MiddlewareHandler } from "hono";
import { logger } from "../telemetry";

// biome-ignore lint/style/useNamingConvention: Hono names the key `Variables`
export type AppEnv = { readonly Variables: { readonly requestId: string } };

const log = logger("http");

const MAX_ID = 128;
// URI-unreserved, starting with a letter or digit, so an id cannot pass for a path or a flag.
const USABLE_ID = /^[A-Za-z0-9][A-Za-z0-9._~-]*$/;

/** Keeps a usable incoming id, else a fresh one. */
function requestIdFor(incoming: string | undefined): string {
  return incoming !== undefined && incoming.length <= MAX_ID && USABLE_ID.test(incoming)
    ? incoming
    : Bun.randomUUIDv7();
}

/**
 * The outermost middleware: sets `x-request-id`, runs the request in the `http` span, and logs
 * one `response` event per request.
 */
export function requestId(): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const id = requestIdFor(c.req.header("x-request-id"));
    c.set("requestId", id);
    await log.withSpan(
      "http",
      { method: c.req.method, path: c.req.path, requestId: id },
      async (span) => {
        const start = performance.now();
        await next();
        c.res.headers.set("x-request-id", id);
        const status = c.res.status;
        span.record({ status });
        const latencyMs = performance.now() - start;
        // Debug keeps stderr to the span's close line while the default file level still gets
        // one line per request. A 5xx is raised to warn so it survives a quieter level.
        log[status >= 500 ? "warn" : "debug"]("response", { status, latencyMs });
        const body = c.res.body;
        if (body !== null) {
          c.res = new Response(
            watchBody(body, (error) => {
              log.error("response failed", {
                requestId: id,
                error,
                latencyMs: performance.now() - start,
              });
            }),
            c.res,
          );
        }
      },
    );
  };
}

// The handler has returned by the time the body streams, so a body error is seen only here.
// A client that hangs up cancels the stream instead, which is not a failure.
function watchBody(
  body: ReadableStream<Uint8Array>,
  onError: (err: unknown) => void,
): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  return new ReadableStream({
    async pull(controller) {
      try {
        const chunk = await reader.read();
        if (chunk.done) {
          controller.close();
        } else {
          controller.enqueue(chunk.value);
        }
      } catch (err) {
        onError(err);
        controller.error(err);
      }
    },
    cancel: (reason) => reader.cancel(reason),
  });
}
