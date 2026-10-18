import { HttpResponse } from "msw";
import { expect, test } from "vitest";
import { call, client } from "@/api-client";
import { NetworkError, ProblemError } from "@/error";
import { createQueryClient } from "@/query";
import { mock } from "@/test/handlers";
import { server } from "@/test/setup";

test("a problem response rejects with a ProblemError", async () => {
  server.use(
    mock(client.health.$get, () =>
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
  const error = await call(client.health.$get()).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ProblemError);
  expect(error).toMatchObject({ status: 404, instance: "req-1", detail: "no such thing" });
});

test("a body that is not a problem still rejects with the status", async () => {
  // A plain Response opts out of the body check, which is what a non-JSON body needs.
  server.use(mock(client.health.$get, () => new Response("Bad Gateway", { status: 502 })));
  const error = await call(client.health.$get()).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ProblemError);
  expect(error).toMatchObject({ status: 502 });
});

test("a 2xx body that is not JSON keeps the parse error", async () => {
  server.use(
    mock(
      client.health.$get,
      () => new Response("{not json", { headers: { "content-type": "application/json" } }),
    ),
  );
  const error = await call(client.health.$get()).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(SyntaxError);
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

test("a handler serves the route's response type", async () => {
  server.use(
    mock(client.health.$get, () =>
      HttpResponse.json({ status: "ok", version: "0.0.0", uptime_s: 1 }),
    ),
  );
  const data = await call(client.health.$get());
  expect(data).toEqual({ status: "ok", version: "0.0.0", uptime_s: 1 });
});

test("a handler cannot return a body the route's response type lacks", () => {
  mock(client.health.$get, () =>
    // @ts-expect-error `uptime` is not a field of `Health`.
    HttpResponse.json({ status: "ok", version: "0.0.0", uptime: 1 }),
  );
});

test("a handler matches a path parameter and types it", async () => {
  server.use(
    mock(client.api.hello[":id"].$get, ({ params }) =>
      HttpResponse.json({ id: params.id, name: "x", created_at: "2026-10-17T00:00:00.000Z" }),
    ),
  );
  const data = await call(client.api.hello[":id"].$get({ param: { id: "abc" } }));
  expect(data.id).toBe("abc");
});

test("a request with no response rejects with a NetworkError", async () => {
  server.use(mock(client.health.$get, () => HttpResponse.error()));
  const error = await call(client.health.$get()).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(NetworkError);
});
