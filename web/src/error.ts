import type { FieldError, Problem } from "@triage/api";

type Described = { message: string; requestId?: string };

/** A failed API response, read from its problem+json body when it has one. */
export class ProblemError extends Error implements Problem {
  override readonly name = "ProblemError";
  readonly type: string;
  readonly status: number;
  readonly title: string;
  readonly detail?: string;
  readonly instance?: string;
  readonly errors?: FieldError[];

  constructor(problem: Problem) {
    super(problem.detail ?? problem.title);
    this.type = problem.type;
    this.status = problem.status;
    this.title = problem.title;
    if (problem.detail !== undefined) this.detail = problem.detail;
    if (problem.instance !== undefined) this.instance = problem.instance;
    if (problem.errors !== undefined) this.errors = problem.errors;
  }

  // A proxy or a crashed daemon can answer with a body that is not a problem.
  static from(body: unknown, response: Response): ProblemError {
    if (isProblem(body)) return new ProblemError(body);
    return new ProblemError({
      type: "about:blank",
      status: response.status,
      title: response.statusText || `HTTP ${response.status}`,
    });
  }
}

/** A request that got no response: the daemon is down or unreachable. */
export class NetworkError extends Error {
  override readonly name = "NetworkError";

  constructor(cause: unknown) {
    super("Cannot reach the triage daemon. Is `triage serve` running?", { cause });
  }
}

function isProblem(body: unknown): body is Problem {
  if (typeof body !== "object" || body === null) return false;
  const fields = body as Record<string, unknown>;
  return (
    typeof fields.type === "string" &&
    typeof fields.status === "number" &&
    typeof fields.title === "string"
  );
}

const FALLBACK = "Something went wrong.";

// The daemon puts the request id in `instance`; an Error may carry it as `requestId` instead.
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
