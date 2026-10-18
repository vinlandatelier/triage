import { z } from "@hono/zod-openapi";

export const BODY_LIMIT = 1 << 20;

const PROBLEM_BASE = "https://triage.studio/problems/";

const CODES = {
  // biome-ignore-start lint/style/useNamingConvention: the codes are the spec's snake_case strings
  not_found: { status: 404, slug: "not-found", title: "Not found" },
  validation: { status: 422, slug: "validation", title: "Validation failed" },
  method_not_allowed: { status: 405, slug: "method-not-allowed", title: "Method not allowed" },
  conflict: { status: 409, slug: "conflict", title: "Conflict" },
  payload_too_large: { status: 413, slug: "payload-too-large", title: "Payload too large" },
  internal: { status: 500, slug: "internal", title: "Internal error" },
  // biome-ignore-end lint/style/useNamingConvention: see above
} as const;

export type ErrorCode = keyof typeof CODES;
type ErrorStatus = (typeof CODES)[ErrorCode]["status"];

const FieldErrorSchema = z.object({ field: z.string(), message: z.string() }).openapi("FieldError");

export type FieldError = z.infer<typeof FieldErrorSchema>;

export const ProblemSchema = z
  .object({
    type: z.url().openapi({ description: "`https://triage.studio/problems/<slug>`" }),
    title: z.string(),
    status: z.number().int().min(100).max(599),
    detail: z.string().optional().openapi({ description: "Present on 4xx only." }),
    instance: z.string().optional().openapi({ description: "The request id." }),
    errors: z
      .array(FieldErrorSchema)
      .optional()
      .openapi({ description: "Present on validation problems only." }),
  })
  .openapi("Problem", { description: "An RFC 9457 problem." });

export type Problem = z.infer<typeof ProblemSchema>;

type AppErrorOptions = ErrorOptions & { readonly errors?: readonly FieldError[] };

/** The one error type handlers throw. `app.onError` renders it as problem+json. */
export class AppError extends Error {
  readonly code: ErrorCode;
  readonly status: ErrorStatus;
  readonly errors?: readonly FieldError[];

  constructor(code: ErrorCode, message: string, options: AppErrorOptions = {}) {
    super(message, options);
    this.name = "AppError";
    this.code = code;
    this.status = CODES[code].status;
    if (options.errors !== undefined) {
      this.errors = options.errors;
    }
  }
}

/** The RFC 9457 body for `err`, `instance` being the request id. */
export function problemBody(err: AppError, instance?: string): Problem {
  const { slug, title } = CODES[err.code];
  return {
    type: `${PROBLEM_BASE}${slug}`,
    title,
    status: err.status,
    // 5xx details would leak internals; the log line carries them instead.
    ...(err.status < 500 ? { detail: err.message } : {}),
    ...(instance === undefined ? {} : { instance }),
    ...(err.code === "validation" ? { errors: [...(err.errors ?? [])] } : {}),
  };
}
