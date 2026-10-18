import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type Config, loadConfig } from "./config";
import { initTelemetry, logger, type Telemetry } from "./telemetry";
import { logFile } from "./telemetry/daily-file";

const log = logger("test");
const temps: string[] = [];
const open: Telemetry[] = [];

afterEach(async () => {
  for (const telemetry of open.splice(0)) {
    await telemetry.close();
  }
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

type Sinks = {
  readonly stderr: string[];
  readonly logs: string;
  readonly telemetry: Telemetry;
  /** Closes telemetry, then parses every JSONL line. */
  readonly lines: () => Promise<Record<string, unknown>[]>;
};

async function start(overrides: Partial<Config> = {}, logs?: string): Promise<Sinks> {
  const dir = mkdtempSync(join(tmpdir(), "triage-telemetry-"));
  temps.push(dir);
  const logsDir = logs ?? join(dir, "logs");
  if (logs === undefined) {
    mkdirSync(logsDir);
  }
  const stderr: string[] = [];
  const config = { ...loadConfig({}), logLevel: "trace", ...overrides } satisfies Config;
  const telemetry = await initTelemetry(
    config,
    { data: dir, logs: logsDir, config: dir },
    { stderr: { write: (line: string) => stderr.push(line) }, ansi: false },
  );
  open.push(telemetry);
  return {
    stderr,
    logs: logsDir,
    telemetry,
    async lines() {
      await telemetry.close();
      return readFileSync(logFile(logsDir, new Date()), "utf8")
        .split("\n")
        .filter((line) => line !== "")
        .map((line) => JSON.parse(line) as Record<string, unknown>);
    },
  };
}

async function onlyLine(sinks: Sinks): Promise<Record<string, unknown>> {
  const lines = await sinks.lines();
  expect(lines).toHaveLength(1);
  return lines[0] ?? {};
}

test("core keys lead the line, then the event's fields", async () => {
  const sinks = await start();
  log.info("hello", { count: 3, ok: true });
  const raw = readFileSync(logFile(sinks.logs, new Date()), "utf8");
  const order = ["ts", "level", "target", "message", "count", "ok"].map((key) =>
    raw.indexOf(`"${key}":`),
  );
  expect(order.every((at) => at >= 0)).toBe(true);
  expect(order).toEqual([...order].sort((a, b) => a - b));

  const line = await onlyLine(sinks);
  expect(line.ts).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/);
  expect(line).toMatchObject({ level: "INFO", target: "triage::test", message: "hello", count: 3 });
  for (const key of ["span", "span_id", "trace_id", "request_id"]) {
    expect(line).not.toHaveProperty(key);
  }
});

test("events inherit request_id and ids from their spans, not span fields", async () => {
  const sinks = await start();
  log.withSpan("request", { requestId: "r1", method: "GET" }, () =>
    log.withSpan("handler", {}, () => log.info("inside", { level: "user" })),
  );
  const line = await onlyLine(sinks);
  expect(line).toMatchObject({ level: "INFO", span: "handler", "field.level": "user" });
  expect(line.request_id).toBe("r1");
  expect(typeof line.span_id).toBe("string");
  expect(line.span_id).not.toBe(line.trace_id);
  expect(line).not.toHaveProperty("method");
});

test("a root span's id is the trace id", async () => {
  const sinks = await start();
  log.withSpan("only", {}, () => log.info("x"));
  const line = await onlyLine(sinks);
  expect(line.span_id).toBe(line.trace_id as string);
});

test("a request_id recorded later is seen", async () => {
  const sinks = await start();
  log.withSpan("request", {}, (span) => {
    span.record({ requestId: "r2" });
    log.info("late");
  });
  expect((await onlyLine(sinks)).request_id).toBe("r2");
});

test("the event's own request_id wins and sits in the tail", async () => {
  const sinks = await start();
  log.withSpan("request", { requestId: "span" }, () => log.info("x", { requestId: "event", z: 1 }));
  const raw = readFileSync(logFile(sinks.logs, new Date()), "utf8");
  const at = (key: string) => raw.indexOf(`"${key}":`);
  expect(at("z")).toBeLessThan(at("span"));
  expect(at("span")).toBeLessThan(at("request_id"));
  expect(at("request_id")).toBeLessThan(at("span_id"));
  expect((await onlyLine(sinks)).request_id).toBe("event");
});

test("a renamed field never duplicates a key", async () => {
  const sinks = await start();
  log.info("x", { ts: 1, "field.ts": 2, message: 3 });
  const raw = readFileSync(logFile(sinks.logs, new Date()), "utf8");
  expect(raw.split('"field.ts":')).toHaveLength(2);
  expect(await onlyLine(sinks)).toMatchObject({
    message: "x",
    "field.ts": 2,
    "field.field.ts": 1,
    "field.message": 3,
  });
});

test("a two-level cause shows as two └ lines on stderr and one joined field in the file", async () => {
  const sinks = await start();
  const err = new Error("save failed", {
    cause: new Error("disk full", { cause: new Error("quota exceeded") }),
  });
  log.error("write", { error: err });
  const out = sinks.stderr.join("");
  expect(out).toContain('write  error="save failed"\n');
  expect(out.match(/└ /g)).toHaveLength(2);
  expect((await onlyLine(sinks)).error).toBe("save failed: disk full: quota exceeded");
});

test("keys turn snake_case and secret keys are redacted", async () => {
  const sinks = await start();
  log.info("x", { latencyMs: 4, password: "hunter2", token: "t", apiToken: "t", setCookie: "c" });
  const line = await onlyLine(sinks);
  expect(line.latency_ms).toBe(4);
  expect(line.api_token).toBe("[redacted]");
  expect(line.set_cookie).toBe("[redacted]");
  expect(line).toMatchObject({ password: "[redacted]", token: "[redacted]" });
  expect(sinks.stderr.join("")).not.toContain("hunter2");
});

test("a span close prints on stderr with its fields and duration, never in the file", async () => {
  const sinks = await start();
  const result = await log.withSpan("hello.create", { requestId: "r3" }, async (span) => {
    await Bun.sleep(1);
    span.record({ status: 201 });
    return 7;
  });
  expect(result).toBe(7);
  const close = sinks.stderr.at(-1) ?? "";
  expect(close).toMatch(
    / INFO {2}test {6}hello\.create {2}status=201 {2}[\d.]+(µs|ms|s) {2}req=r3\n$/,
  );
  expect(await sinks.lines()).toEqual([]);
});

test("a span closes and rethrows when its function throws", async () => {
  const sinks = await start();
  expect(() =>
    log.withSpan("boom", {}, () => {
      throw new Error("no");
    }),
  ).toThrow("no");
  await expect(
    log.withSpan("later", {}, async () => {
      await Bun.sleep(0);
      throw new Error("async no");
    }),
  ).rejects.toThrow("async no");
  expect(sinks.stderr.map((line) => line.split(/\s+/)[3])).toEqual(["boom", "later"]);
});

test("levels filter each sink on its own", async () => {
  const sinks = await start({ logLevel: "warn", logFileLevel: "info" });
  log.debug("dropped everywhere");
  log.info("file only");
  log.warn("both");
  log.withSpan("quiet", {}, () => undefined);
  expect(sinks.stderr).toHaveLength(1);
  expect(sinks.stderr[0]).toContain("both");
  expect((await sinks.lines()).map((line) => line.message)).toEqual(["file only", "both"]);
});

test("json writes the file format to stderr and no span closes", async () => {
  const sinks = await start({ logFormat: "json" });
  log.withSpan("s", {}, () => log.info("x"));
  const [line] = await sinks.lines();
  expect(sinks.stderr).toEqual([`${JSON.stringify(line)}\n`]);
});

test("logging after close is a no-op", async () => {
  const sinks = await start();
  await sinks.telemetry.close();
  log.error("late");
  expect(sinks.stderr).toEqual([]);
  expect(await sinks.lines()).toEqual([]);
});

test("an unusable logs dir gives one warning naming it and logs to stderr only", async () => {
  const dir = mkdtempSync(join(tmpdir(), "triage-telemetry-"));
  temps.push(dir);
  const file = join(dir, "logs");
  writeFileSync(file, "");
  const sinks = await start({}, file);
  log.info("still here");
  expect(sinks.stderr).toHaveLength(2);
  expect(sinks.stderr[0]).toContain(`WARN  telemetry  cannot write log files in ${file}`);
  expect(sinks.stderr[1]).toContain("still here");
});

test("a read-only logs dir is unusable", async () => {
  const dir = mkdtempSync(join(tmpdir(), "triage-telemetry-"));
  temps.push(dir);
  chmodSync(dir, 0o500);
  try {
    // Root ignores the mode bits, so there is nothing to test.
    writeFileSync(join(dir, "root-check"), "");
    return;
  } catch {
    // Expected for a normal user.
  }
  const sinks = await start({}, dir);
  chmodSync(dir, 0o700);
  expect(sinks.stderr.join("")).toContain("cannot write log files");
});

test("startup warnings from config are logged once telemetry runs", async () => {
  const sinks = await start({ warnings: ["OTEL_EXPORTER_OTLP_ENDPOINT: bad; OTLP export is off"] });
  expect(sinks.stderr.join("")).toContain("OTLP export is off");
  expect((await sinks.lines())[0]?.level).toBe("WARN");
});

test("withSpan exports an OTel span when an endpoint is set", async () => {
  const bodies: Uint8Array[] = [];
  const collector = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(request) {
      bodies.push(new Uint8Array(await request.arrayBuffer()));
      return new Response(null, { status: 200 });
    },
  });
  try {
    const url = new URL(`http://127.0.0.1:${collector.port}`);
    const sinks = await start({
      otelEndpoint: { url, traces: new URL("/v1/traces", url), var: "TRIAGE_OTEL_ENDPOINT" },
    });
    log.withSpan("probe.span", { requestId: "r9" }, () => undefined);
    await sinks.telemetry.close();
    const body = new TextDecoder().decode(Buffer.concat(bodies));
    expect(body).toContain("probe.span");
    expect(body).toContain("service.name");
    expect(body).toContain("r9");
  } finally {
    await collector.stop(true);
  }
});

test("close finishes when the collector refuses the final export", async () => {
  const server = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response() });
  const url = new URL(`http://127.0.0.1:${server.port}`);
  await server.stop(true);
  const sinks = await start({
    otelEndpoint: { url, traces: new URL("/v1/traces", url), var: "TRIAGE_OTEL_ENDPOINT" },
  });
  log.withSpan("refused", {}, () => undefined);
  await sinks.telemetry.close();
  log.error("after close");
  expect(sinks.stderr.join("")).not.toContain("after close");
  expect(sinks.stderr.join("")).toContain("cannot export traces");
}, 15_000);
