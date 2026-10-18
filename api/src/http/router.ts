import { OpenAPIHono } from "@hono/zod-openapi";
import type { ZodError } from "zod";
import { AppError, type FieldError } from "../error";
import type { AppEnv } from "./request-id";

/**
 * An `OpenAPIHono` whose validators throw a 422 `validation` error. Every feature builds its
 * routes on one, because the hook is per instance and `app.route` does not pass it down.
 */
export function createRouter(): OpenAPIHono<AppEnv> {
  return new OpenAPIHono<AppEnv>({
    defaultHook: (result) => {
      if (!result.success) {
        throw validationError(result.error, result.target);
      }
      return undefined;
    },
  });
}

const TARGET_FIELDS: Readonly<Record<string, string>> = { json: "body", form: "body" };

function validationError(error: ZodError, target: string): AppError {
  const errors: FieldError[] = error.issues.map((issue) => ({
    field:
      issue.path.length === 0
        ? (TARGET_FIELDS[target] ?? target)
        : issue.path.map(String).join("."),
    message: issue.message,
  }));
  return new AppError("validation", "the request is invalid", { errors });
}
