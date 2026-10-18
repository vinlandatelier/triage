import { expect, test } from "bun:test";
import { AppError, problemBody } from "./error";

test("a 4xx problem carries its detail", () => {
  expect(problemBody(new AppError("not_found", "no item 7"), "req-1")).toEqual({
    type: "https://triage.studio/problems/not-found",
    title: "Not found",
    status: 404,
    detail: "no item 7",
    instance: "req-1",
  });
});

test("a validation problem lists its field errors", () => {
  const errors = [{ field: "name", message: "must not be empty" }];
  expect(problemBody(new AppError("validation", "the request is invalid", { errors }))).toEqual({
    type: "https://triage.studio/problems/validation",
    title: "Validation failed",
    status: 422,
    detail: "the request is invalid",
    errors,
  });
});

test("an internal problem hides its detail", () => {
  const err = new AppError("internal", "query failed", { cause: new Error("password=hunter2") });
  expect(problemBody(err)).toEqual({
    type: "https://triage.studio/problems/internal",
    title: "Internal error",
    status: 500,
  });
});
