import { afterEach, expect, test } from "bun:test";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { testClient } from "hono/testing";
import { loadConfig } from "../../config";
import { createApp } from "../../http/app";
import { openTempStore } from "../../store/temp";
import { initTelemetry } from "../../telemetry";

const disposers: (() => void | Promise<void>)[] = [];

afterEach(async () => {
  for (const dispose of disposers.splice(0).reverse()) {
    await dispose();
  }
});

function spawnApp() {
  const temp = openTempStore();
  disposers.push(temp.dispose);
  const app = createApp({ startedAt: performance.now(), store: temp.store });
  return { app, client: testClient(app), store: temp.store };
}

const REDACTED = new Map([
  ["id", "[id]"],
  ["created_at", "[ts]"],
  ["updated_at", "[ts]"],
  ["instance", "[request id]"],
]);

/** Blanks what changes per run, at the top level of an object or of each array item. */
function redacted(body: unknown): unknown {
  if (Array.isArray(body)) {
    return body.map(redacted);
  }
  if (typeof body !== "object" || body === null) {
    return body;
  }
  return Object.fromEntries(
    Object.entries(body).map(([key, value]) => [key, REDACTED.get(key) ?? value]),
  );
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

test("create returns 201 with the trimmed name", async () => {
  const { client } = spawnApp();
  const response = await client.api.hello.$post({ json: { name: "  world  " } });
  expect(response.status).toBe(201);
  const body = await response.json();
  expect(body.created_at).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{1,3})?Z$/);
  expect(redacted(body)).toMatchSnapshot();
});

test("get returns the created row", async () => {
  const { client } = spawnApp();
  const created = await (await client.api.hello.$post({ json: { name: "world" } })).json();
  const response = await client.api.hello[":id"].$get({ param: { id: created.id } });
  expect(response.status).toBe(200);
  expect(await response.json()).toEqual(created);
});

test("an id in lowercase finds the row, as ULIDs are case-insensitive", async () => {
  const { client } = spawnApp();
  const created = await (await client.api.hello.$post({ json: { name: "world" } })).json();
  const response = await client.api.hello[":id"].$get({ param: { id: created.id.toLowerCase() } });
  expect(await response.json()).toEqual(created);
});

test("an empty table lists []", async () => {
  const { client } = spawnApp();
  const response = await client.api.hello.$get();
  expect(response.status).toBe(200);
  expect(await response.json()).toEqual([]);
});

test("list returns the 20 newest first", async () => {
  const { client } = spawnApp();
  for (let n = 0; n < 25; n++) {
    const response = await client.api.hello.$post({ json: { name: `hello ${n}` } });
    expect(response.status).toBe(201);
    // ULIDs are random within one millisecond, so a tie would order by chance.
    await sleep(2);
  }
  const response = await client.api.hello.$get();
  expect(response.status).toBe(200);
  const body = await response.json();
  expect(body.map((hello) => hello.name)).toEqual(
    Array.from({ length: 20 }, (_, i) => `hello ${24 - i}`),
  );
  expect(redacted(body)).toMatchSnapshot();
});

test("unknown and malformed ids are 404 problems", async () => {
  const { client } = spawnApp();
  for (const id of ["01ARZ3NDEKTSV4RRFFQ69G5FAV", "01JUNK", "not-a-ulid"]) {
    const response = await client.api.hello[":id"].$get({ param: { id } });
    expect<number>(response.status).toBe(404);
    expect(response.headers.get("content-type")).toBe("application/problem+json");
    const body = await response.json();
    expect(typeof (body as { instance?: unknown }).instance).toBe("string");
    expect(redacted(body)).toMatchSnapshot(`not found ${id}`);
  }
});

test("a blank or missing name is a 422 naming name, and writes no row", async () => {
  const { app, client } = spawnApp();
  const cases = { empty: { name: "   " }, missing: {} };
  for (const [name, json] of Object.entries(cases)) {
    const response = await app.request("/api/hello", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(json),
    });
    expect(response.status).toBe(422);
    const body = await response.json();
    expect(body.errors[0].field).toBe("name");
    expect(redacted(body)).toMatchSnapshot(`invalid ${name}`);
  }
  expect(await (await client.api.hello.$get()).json()).toEqual([]);
});

test("a name over 80 characters or with a control character is a 422", async () => {
  const { client } = spawnApp();
  const cases = [
    ["a".repeat(81), "must be at most 80 characters"],
    ["a\0b", "must not contain control characters"],
    ["a\nb", "must not contain control characters"],
  ] as const;
  for (const [name, message] of cases) {
    const response = await client.api.hello.$post({ json: { name } });
    expect<number>(response.status).toBe(422);
    expect(await response.json()).toMatchObject({ errors: [{ field: "name", message }] });
  }
  const ok = await client.api.hello.$post({ json: { name: "é".repeat(80) } });
  expect(ok.status).toBe(201);
});

test("the name is trimmed of Unicode White_Space, as Rust's trim does", async () => {
  const { client } = spawnApp();
  const nel = await client.api.hello.$post({ json: { name: "\u0085 a\u3000" } });
  expect(await nel.json()).toMatchObject({ name: "a" });
  const bom = await client.api.hello.$post({ json: { name: "\ufeffa" } });
  expect(await bom.json()).toMatchObject({ name: "\ufeffa" });
});

test("create logs the new id in the hello.create span", async () => {
  const { client, store } = spawnApp();
  const dir = join(store.db.$client.filename, "..");
  const logs = join(dir, "logs");
  // `createPaths` makes the logs dir at startup; this test skips it.
  mkdirSync(logs);
  const telemetry = await initTelemetry(
    loadConfig({}),
    { data: dir, logs, config: dir },
    { stderr: { write: () => undefined }, ansi: false },
  );
  let created: { id: string };
  try {
    created = await (await client.api.hello.$post({ json: { name: "world" } })).json();
  } finally {
    // Flushes the file.
    await telemetry.close();
  }

  const lines = readdirSync(logs)
    .flatMap((file) => readFileSync(join(logs, file), "utf8").split("\n"))
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line) as Record<string, unknown>);
  expect(lines).toContainEqual(
    expect.objectContaining({
      message: "hello created",
      span: "hello.create",
      "hello.id": created.id,
    }),
  );
});
