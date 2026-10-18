import { createRoute, type OpenAPIHono, z } from "@hono/zod-openapi";
import type { Context } from "hono";
import { bodyLimit } from "hono/body-limit";
import { HTTPException } from "hono/http-exception";
import { methodNotAllowed } from "hono/method-not-allowed";
import packageJson from "../../package.json" with { type: "json" };
import { AppError, BODY_LIMIT, ProblemSchema, problemBody } from "../error";
import { createHelloRoutes } from "../features/hello/http";
import type { Store } from "../store/open";
import { logger } from "../telemetry";
import { type AppEnv, requestId } from "./request-id";
import { createRouter } from "./router";

// `bun build --define TRIAGE_COMPILED=true` sets it for the executable; from source it is unset.
declare const TRIAGE_COMPILED: boolean | undefined;

export type AppDeps = {
  /** `performance.now()` when the server started. */
  readonly startedAt: number;
  readonly store: Store;
};

const log = logger("http");

const PROBLEM_CONTENT_TYPE = "application/problem+json";
const PROBLEM_RESPONSE = { $ref: "#/components/responses/Problem" } as const;

export const HealthSchema = z
  .object({
    status: z.literal("ok"),
    version: z.string(),
    // biome-ignore lint/style/useNamingConvention: the wire field is snake_case
    uptime_s: z.number().int().nonnegative(),
  })
  .openapi("Health");

const healthRoute = createRoute({
  method: "get",
  path: "/health",
  operationId: "health_get",
  summary: "Daemon health",
  description: "The version and uptime of the running daemon; `triage status` reads it.",
  responses: {
    200: {
      description: "The daemon is up",
      content: { "application/json": { schema: HealthSchema } },
    },
  },
});

/** The daemon's one Hono app. The returned value carries every route's type, for `hc` and tests. */
export function createApp(deps: AppDeps) {
  const app = createRouter();
  app.use(requestId());
  app.use(async (_c, next) => {
    try {
      await next();
    } catch (err) {
      // Hono hands only `Error` values to `onError`; anything else would escape `fetch`.
      throw err instanceof Error
        ? err
        : new AppError("internal", "a handler threw a non-Error value", { cause: err });
    }
  });
  app.use(
    methodNotAllowed({
      app,
      onMethodNotAllowed: (c, methods) =>
        problemResponse(
          c,
          new AppError("method_not_allowed", `${c.req.method} is not allowed on ${c.req.path}`),
          { allow: methods.join(", ") },
        ),
    }),
  );
  app.use(
    bodyLimit({
      maxSize: BODY_LIMIT,
      onError: () => {
        throw new AppError(
          "payload_too_large",
          `the request body is larger than ${BODY_LIMIT} bytes`,
        );
      },
    }),
  );
  app.onError((err, c) => {
    const appError = toAppError(err);
    if (appError.code === "internal") {
      log.error("internal error", { error: err });
    }
    return problemResponse(c, appError);
  });
  app.notFound((c) => problemResponse(c, new AppError("not_found", `no route for ${c.req.path}`)));

  const routes = app
    .openapi(healthRoute, (c) =>
      c.json(
        {
          status: "ok" as const,
          version: packageJson.version,
          // biome-ignore lint/style/useNamingConvention: the wire field is snake_case
          uptime_s: Math.floor((performance.now() - deps.startedAt) / 1000),
        },
        200,
      ),
    )
    .route("/", createHelloRoutes(deps));

  app.openAPIRegistry.register("Problem", ProblemSchema);
  app.openAPIRegistry.registerComponent("responses", "Problem", {
    description: "An RFC 9457 problem",
    content: { [PROBLEM_CONTENT_TYPE]: { schema: { $ref: "#/components/schemas/Problem" } } },
  });
  let doc: object | undefined;
  const document = () => {
    doc ??= openApiDocument(app);
    return doc;
  };
  app.get("/api/openapi.json", (c) => c.json(document()));
  if (typeof TRIAGE_COMPILED === "undefined" || !TRIAGE_COMPILED) {
    // A dynamic import inside the dead branch keeps Scalar out of the executable.
    app.get("/api/scalar", async (c, next) => {
      const { Scalar } = await import("@scalar/hono-api-reference");
      return Scalar<AppEnv>({ content: document() })(c, next);
    });
  }
  return routes;
}

export type AppType = ReturnType<typeof createApp>;
// For the web app, which imports only this file's types.
export type { FieldError, Problem } from "../error";

/** The OpenAPI 3.1 document, with the problem response as every operation's `default`. */
export function openApiDocument(
  app: OpenAPIHono<AppEnv>,
): ReturnType<OpenAPIHono["getOpenAPI31Document"]> {
  const doc = app.getOpenAPI31Document({
    openapi: "3.1.0",
    info: { title: "Triage", version: packageJson.version },
    // No host, so a client resolves paths against wherever it loaded from.
    servers: [{ url: "/" }],
  });
  for (const item of Object.values(doc.paths ?? {})) {
    for (const operation of Object.values(item)) {
      if (typeof operation === "object" && operation !== null && "responses" in operation) {
        operation.responses.default ??= PROBLEM_RESPONSE;
      }
    }
  }
  return doc;
}

function problemResponse(
  c: Context<AppEnv>,
  err: AppError,
  headers: Record<string, string> = {},
): Response {
  return c.body(JSON.stringify(problemBody(err, c.get("requestId"))), err.status, {
    ...headers,
    "content-type": PROBLEM_CONTENT_TYPE,
  });
}

function toAppError(err: Error): AppError {
  if (err instanceof AppError) {
    return err;
  }
  if (err instanceof HTTPException) {
    // The validators throw 400 for malformed JSON and 415 for a body that is not JSON.
    if (err.status === 400 || err.status === 415) {
      return new AppError("validation", "the request is invalid", {
        errors: [{ field: "body", message: err.message }],
        cause: err,
      });
    }
    if (err.status === 413) {
      return new AppError("payload_too_large", err.message, { cause: err });
    }
  }
  return new AppError("internal", "internal error", { cause: err });
}
