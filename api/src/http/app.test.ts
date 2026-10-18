import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRoute, z } from "@hono/zod-openapi";
import { testClient } from "hono/testing";
import packageJson from "../../package.json" with { type: "json" };
import { loadConfig } from "../config";
import { AppError, BODY_LIMIT, ProblemSchema, problemBody } from "../error";
import { openTempStore } from "../store/temp";
import { initTelemetry, type Telemetry } from "../telemetry";
import { type AppType, createApp, openApiDocument } from "./app";

const open: Telemetry[] = [];
const temps: string[] = [];
const stores: (() => void)[] = [];

afterEach(async () => {
  for (const dispose of stores.splice(0)) {
    dispose();
  }
  for (const telemetry of open.splice(0)) {
    await telemetry.close();
  }
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function app(): AppType {
  const { store, dispose } = openTempStore();
  stores.push(dispose);
  return createApp({ startedAt: performance.now(), store });
}

/** JSON log lines, written to a captured stderr. */
async function captureLogs(): Promise<Record<string, unknown>[]> {
  const dir = mkdtempSync(join(tmpdir(), "triage-http-"));
  temps.push(dir);
  const lines: Record<string, unknown>[] = [];
  const config = { ...loadConfig({}), logLevel: "debug", logFormat: "json" } as const;
  open.push(
    await initTelemetry(
      config,
      { data: dir, logs: join(dir, "logs"), config: dir },
      { stderr: { write: (line: string) => lines.push(JSON.parse(line)) }, ansi: false },
    ),
  );
  return lines;
}

async function problem(response: Response) {
  expect(response.headers.get("content-type")).toBe("application/problem+json");
  const body = ProblemSchema.strict().parse(await response.json());
  expect(body.instance).toBe(response.headers.get("x-request-id") ?? "missing");
  return body;
}

test("health reports ok, the version, and whole seconds of uptime", async () => {
  const response = await testClient(app()).health.$get();
  expect(response.status).toBe(200);
  expect(response.headers.get("content-type")).toStartWith("application/json");
  const body = await response.json();
  expect(body.status).toBe("ok");
  expect(body.version).toBe(packageJson.version);
  expect(Number.isInteger(body.uptime_s)).toBe(true);
});

test("HEAD on a GET route is answered", async () => {
  const response = await app().request("/health", { method: "HEAD" });
  expect(response.status).toBe(200);
  expect(await response.text()).toBe("");
});

test("a wrong method is a 405 problem listing the path's methods", async () => {
  const response = await app().request("/health", { method: "POST" });
  expect(response.status).toBe(405);
  expect(response.headers.get("allow")).toBe("GET, HEAD");
  expect(await problem(response)).toMatchObject({
    type: "https://triage.studio/problems/method-not-allowed",
    detail: "POST is not allowed on /health",
  });
});

test("405 matches `:param` routes the way the router does", async () => {
  const thing = app();
  thing.get("/api/things/:id", (c) => c.text(c.req.param("id")));
  expect((await thing.request("/api/things/7")).status).toBe(200);
  const wrong = await thing.request("/api/things/7", { method: "DELETE" });
  expect(wrong.status).toBe(405);
  expect(wrong.headers.get("allow")).toBe("GET, HEAD");
  for (const path of ["/api/things", "/api/things/7/more"]) {
    expect((await thing.request(path, { method: "DELETE" })).status).toBe(404);
  }
});

test("unknown paths are 404 problems, inside /api and out", async () => {
  for (const path of ["/api/nope", "/nope", "/api/scalar/x"]) {
    const response = await app().request(path);
    expect(response.status).toBe(404);
    expect(await problem(response)).toEqual({
      type: "https://triage.studio/problems/not-found",
      title: "Not found",
      status: 404,
      detail: `no route for ${path}`,
      instance: expect.any(String),
    });
  }
});

test("a usable incoming request id is kept and becomes the instance", async () => {
  for (const id of ["from-client", "a", "0._~-", "x".repeat(128)]) {
    const response = await app().request("/nope", { headers: { "x-request-id": id } });
    expect(response.headers.get("x-request-id")).toBe(id);
    expect((await problem(response)).instance).toBe(id);
  }
});

test("an unusable incoming request id is replaced with a UUIDv7", async () => {
  for (const id of ["x".repeat(129), "a/b", "-a", ".a", "a b", "é"]) {
    const response = await app().request("/health", { headers: { "x-request-id": id } });
    expect(response.headers.get("x-request-id")).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  }
});

test("each request logs one http span and a response event under its id", async () => {
  const lines = await captureLogs();
  const response = await app().request("/health?x=1");
  const id = response.headers.get("x-request-id");
  const event = lines.find((line) => line.message === "response");
  expect(event).toMatchObject({
    level: "DEBUG",
    target: "triage::http",
    span: "http",
    // biome-ignore lint/style/useNamingConvention: JSONL keys are snake_case
    request_id: id,
    status: 200,
  });
  expect(typeof event?.latency_ms).toBe("number");
});

test("a 5xx raises the response event to warn", async () => {
  const lines = await captureLogs();
  const failing = app();
  failing.get("/boom", () => {
    throw new Error("boom");
  });
  await failing.request("/boom");
  expect(lines.find((line) => line.message === "response")?.level).toBe("WARN");
});

test("a thrown Error is an internal 500 with no detail, logged with its cause chain", async () => {
  const lines = await captureLogs();
  const failing = app();
  failing.get("/boom", () => {
    throw new Error("query failed", { cause: new Error("password=hunter2") });
  });
  const response = await failing.request("/boom");
  expect(response.status).toBe(500);
  const body = await problem(response);
  expect(body).toEqual({
    type: "https://triage.studio/problems/internal",
    title: "Internal error",
    status: 500,
    instance: expect.any(String),
  });
  const logged = lines.find((line) => line.message === "internal error");
  expect(logged).toMatchObject({
    level: "ERROR",
    error: "query failed: password=hunter2",
    // biome-ignore lint/style/useNamingConvention: JSONL keys are snake_case
    request_id: body.instance,
  });
});

test("an internal AppError logs its own message and its cause", async () => {
  const lines = await captureLogs();
  const failing = app();
  failing.get("/boom", () => {
    throw new AppError("internal", "sync failed", { cause: new Error("disk full") });
  });
  expect((await failing.request("/boom")).status).toBe(500);
  expect(lines.find((line) => line.message === "internal error")?.error).toBe(
    "sync failed: disk full",
  );
});

test("a body that fails mid-stream logs response failed under the request id", async () => {
  const lines = await captureLogs();
  const failing = app();
  failing.get("/stream", (c) =>
    c.body(
      new ReadableStream({
        pull(controller) {
          controller.error(new Error("disk gone"));
        },
      }),
    ),
  );
  const response = await failing.request("/stream");
  await expect(response.text()).rejects.toThrow("disk gone");
  await Bun.sleep(0);
  expect(lines.find((line) => line.message === "response failed")).toMatchObject({
    level: "ERROR",
    error: "disk gone",
    // biome-ignore lint/style/useNamingConvention: JSONL keys are snake_case
    request_id: response.headers.get("x-request-id"),
  });
});

test("a thrown string is an internal 500 with no detail", async () => {
  const failing = app();
  failing.get("/boom", () => {
    throw "boom";
  });
  const response = await failing.request("/boom");
  expect(response.status).toBe(500);
  expect((await problem(response)).detail).toBeUndefined();
});

test("a 4xx AppError keeps its detail", async () => {
  const failing = app();
  failing.get("/taken", () => {
    throw new AppError("conflict", "name is taken");
  });
  const response = await failing.request("/taken");
  expect(response.status).toBe(409);
  expect(await problem(response)).toMatchObject({ title: "Conflict", detail: "name is taken" });
});

function withBodyRoute() {
  const thing = app();
  thing.openapi(
    createRoute({
      method: "post",
      path: "/api/things",
      operationId: "things_create",
      request: {
        body: {
          required: true,
          content: {
            "application/json": {
              schema: z.strictObject({ name: z.string().min(1), tags: z.array(z.string()) }),
            },
          },
        },
      },
      responses: { 204: { description: "Created" } },
    }),
    (c) => c.body(null, 204),
  );
  return thing;
}

function post(body: BodyInit, headers: Record<string, string> = {}): RequestInit {
  return { method: "POST", body, headers: { "content-type": "application/json", ...headers } };
}

test("a validation failure is a 422 problem with field paths", async () => {
  const response = await withBodyRoute().request(
    "/api/things",
    post(JSON.stringify({ name: "", tags: [1] })),
  );
  expect(response.status).toBe(422);
  const body = await problem(response);
  expect(body).toMatchObject({
    type: "https://triage.studio/problems/validation",
    detail: "the request is invalid",
  });
  expect(body.errors?.map((error) => error.field)).toEqual(["name", "tags.0"]);
});

test("a malformed or non-JSON body is a 422 on field body", async () => {
  for (const init of [post("{"), post("name=x", { "content-type": "text/plain" })]) {
    const response = await withBodyRoute().request("/api/things", init);
    expect(response.status).toBe(422);
    expect((await problem(response)).errors?.map((error) => error.field)).toEqual(["body"]);
  }
});

test("a body over 1 MiB is a 413 problem, on known and unknown routes", async () => {
  const big = "x".repeat(2 * BODY_LIMIT);
  for (const path of ["/api/things", "/api/nope"]) {
    const response = await withBodyRoute().request(
      path,
      post(big, { "content-length": String(big.length) }),
    );
    expect(response.status).toBe(413);
    expect((await problem(response)).type).toBe("https://triage.studio/problems/payload-too-large");
  }
});

test("the OpenAPI document is served and pinned", async () => {
  const served = await (await app().request("/api/openapi.json")).json();
  expect(served).toEqual(JSON.parse(JSON.stringify(openApiDocument(app()))));
  expect(served).toMatchSnapshot();
});

test("every operation has a feature_verb id and a problem default", () => {
  const doc = openApiDocument(app());
  expect(Object.keys(doc.paths ?? {})).toContain("/health");
  for (const [path, item] of Object.entries(doc.paths ?? {})) {
    for (const [method, operation] of Object.entries(item)) {
      expect(operation, `${method} ${path}`).toMatchObject({
        operationId: expect.stringMatching(/^[a-z]+_[a-z]+$/),
        responses: { default: { $ref: "#/components/responses/Problem" } },
      });
    }
  }
});

test("the Problem schema names every key a problem body can have", () => {
  const doc = openApiDocument(app());
  const schema = doc.components?.schemas?.Problem;
  const full = problemBody(
    new AppError("validation", "bad", { errors: [{ field: "name", message: "required" }] }),
    "req-1",
  );
  expect(Object.keys(schema && "properties" in schema ? (schema.properties ?? {}) : {})).toEqual(
    Object.keys(full),
  );
  const minimal = problemBody(new AppError("internal", "boom"));
  expect(schema && "required" in schema ? schema.required : []).toEqual(Object.keys(minimal));
});

test("Scalar renders every route when running from source", async () => {
  const response = await app().request("/api/scalar");
  expect(response.status).toBe(200);
  expect(response.headers.get("content-type")).toBe("text/html; charset=UTF-8");
  expect(await response.text()).toContain('"/health"');
});
