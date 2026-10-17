import { HttpResponse, http } from "msw";
import { expect, test } from "vitest";
import { healthGet } from "@/api";
import { ProblemError } from "@/error";
import { createQueryClient } from "@/query";
import { server } from "@/test/setup";

const health = `${window.location.origin}/api/health`;

test("a problem response rejects with a ProblemError", async () => {
  server.use(
    http.get(health, () =>
      HttpResponse.json(
        {
          type: "https://triage.studio/problems/not-found",
          title: "Not Found",
          status: 404,
          detail: "no such thing",
          instance: "req-1",
        },
        { status: 404, headers: { "content-type": "application/problem+json" } },
      ),
    ),
  );
  const error = await healthGet().catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ProblemError);
  expect(error).toMatchObject({ status: 404, instance: "req-1", detail: "no such thing" });
});

test("a body that is not a problem still rejects with the status", async () => {
  server.use(http.get(health, () => new HttpResponse("Bad Gateway", { status: 502 })));
  const error = await healthGet().catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ProblemError);
  expect(error).toMatchObject({ status: 502 });
});

test("queries throw 5xx to the error boundary and keep 4xx local", () => {
  const throwOnError = createQueryClient().getDefaultOptions().queries?.throwOnError;
  if (typeof throwOnError !== "function") throw new Error("throwOnError is not a predicate");
  const problem = (status: number) => new ProblemError({ type: "t", title: "t", status });
  // biome-ignore lint/suspicious/noExplicitAny: the predicate ignores its query argument.
  const query = {} as any;
  expect(throwOnError(problem(500), query)).toBe(true);
  expect(throwOnError(problem(404), query)).toBe(false);
});
