import { expect, test } from "bun:test";
import {
  ConfigError,
  configEntries,
  loadConfig,
  pathEnv,
  preflight,
  redactUserinfo,
  secret,
  tracesUrl,
} from "./config";

// Env names break the naming rule as object keys, so tests build env from pairs.
function vars(...pairs: [string, string][]): Record<string, string> {
  return Object.fromEntries(pairs);
}

function loadError(env: Record<string, string>): ConfigError {
  try {
    loadConfig(env);
  } catch (err) {
    if (err instanceof ConfigError) {
      return err;
    }
    throw err;
  }
  throw new Error("loadConfig must fail");
}

test("defaults without env", () => {
  expect(loadConfig({})).toEqual({
    port: 7311,
    logLevel: "info",
    logFileLevel: "debug",
    logFormat: "columns",
    warnings: [],
  });
});

test("reads prefixed env", () => {
  const config = loadConfig(
    vars(
      ["TRIAGE_PORT", "8000"],
      ["TRIAGE_LOG_FORMAT", "json"],
      ["TRIAGE_LOG_LEVEL", "WARN"],
      ["TRIAGE_OTEL_ENDPOINT", "http://localhost:4317"],
    ),
  );
  expect(config.port).toBe(8000);
  expect(config.logFormat).toBe("json");
  expect(config.logLevel).toBe("warn");
  expect(config.otelEndpoint?.traces.href).toBe("http://localhost:4317/v1/traces");
});

test.each([
  ["+80", 80],
  [" 080 ", 80],
])("TRIAGE_PORT=%p parses like Rust", (port, want) => {
  expect(loadConfig(vars(["TRIAGE_PORT", port])).port).toBe(want);
});

test("empty values mean unset and unknown TRIAGE_ vars are ignored", () => {
  expect(
    loadConfig(vars(["TRIAGE_PORT", ""], ["TRIAGE_OTEL_ENDPOINT", ""], ["TRIAGE_UNKNOWN", "x"])),
  ).toEqual(loadConfig({}));
});

test.each(["0", "70000", "abc", "-1", "1.5", "99999999999999999999"])(
  "TRIAGE_PORT=%s exits 2 naming the var",
  (port) => {
    const err = loadError(vars(["TRIAGE_PORT", port]));
    expect(err.exitCode).toBe(2);
    expect(err.message).toBe(
      "invalid environment variables: TRIAGE_PORT: expected a port from 1 to 65535",
    );
  },
);

test("invalid log levels exit 2 naming each var", () => {
  const err = loadError(
    vars(["TRIAGE_LOG_LEVEL", "triage=debug,tower=info"], ["TRIAGE_LOG_FILE_LEVEL", "loud"]),
  );
  expect(err.exitCode).toBe(2);
  expect(err.message).toContain("TRIAGE_LOG_LEVEL:");
  expect(err.message).toContain("TRIAGE_LOG_FILE_LEVEL:");
});

test.each(["pretty", "compact", "Columns"])("TRIAGE_LOG_FORMAT=%s exits 2", (format) => {
  const err = loadError(vars(["TRIAGE_LOG_FORMAT", format]));
  expect(err.exitCode).toBe(2);
  expect(err.message).toContain("TRIAGE_LOG_FORMAT: use columns or json");
});

test("invalid values are not echoed", () => {
  const err = loadError(vars(["TRIAGE_PORT", "hunter2"], ["TRIAGE_OTEL_ENDPOINT", "hunter3"]));
  expect(err.message).toContain("TRIAGE_PORT");
  expect(err.message).toContain("TRIAGE_OTEL_ENDPOINT");
  expect(err.message).not.toContain("hunter");
});

test.each(["not a url", "otlp://user:secret@host", "localhost:4317"])(
  "TRIAGE_OTEL_ENDPOINT=%s exits 2",
  (value) => {
    const err = loadError(vars(["TRIAGE_OTEL_ENDPOINT", value]));
    expect(err.exitCode).toBe(2);
    expect(err.message).toContain("TRIAGE_OTEL_ENDPOINT: expected an http URL with a host");
    expect(err.message).not.toContain("secret");
  },
);

test("https TRIAGE_OTEL_ENDPOINT exits 2", () => {
  expect(loadError(vars(["TRIAGE_OTEL_ENDPOINT", "https://collector:4318"])).message).toContain(
    "https is not supported",
  );
});

test("otel endpoint falls back to the standard var, and TRIAGE_OTEL_ENDPOINT wins", () => {
  const standard = vars(["OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"]);
  expect(loadConfig(standard).otelEndpoint?.traces.href).toBe("http://collector:4318/v1/traces");
  expect(
    loadConfig({ ...standard, ...vars(["TRIAGE_OTEL_ENDPOINT", "http://triage:4318"]) })
      .otelEndpoint?.traces.href,
  ).toBe("http://triage:4318/v1/traces");
});

test("empty TRIAGE_OTEL_ENDPOINT disables OTLP", () => {
  expect(
    loadConfig(
      vars(["TRIAGE_OTEL_ENDPOINT", ""], ["OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"]),
    ),
  ).toEqual(loadConfig({}));
});

test("traces var wins over the base and is used verbatim", () => {
  const config = loadConfig(
    vars(
      ["OTEL_EXPORTER_OTLP_ENDPOINT", "http://base:4318"],
      ["OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "http://traces:4318/custom"],
    ),
  );
  expect(config.otelEndpoint?.traces.href).toBe("http://traces:4318/custom");
});

test.each([
  ["OTEL_EXPORTER_OTLP_ENDPOINT", "not a url"],
  ["OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "https://user:hunter2@collector:4318"],
])("malformed %s warns and leaves OTLP off", (name, value) => {
  const config = loadConfig(vars([name, value]));
  expect(config.otelEndpoint).toBeUndefined();
  expect(config.warnings).toHaveLength(1);
  expect(config.warnings[0]).toContain(name);
  expect(config.warnings[0]).not.toContain("hunter2");
});

test.each([
  ["http://h:4318", "http://h:4318/v1/traces"],
  ["http://h:4318/otlp", "http://h:4318/otlp/v1/traces"],
  ["http://h:4318/otlp/", "http://h:4318/otlp/v1/traces"],
])("traces path is appended to %s", (base, want) => {
  expect(tracesUrl(new URL(base)).href).toBe(want);
});

test("preflight lists every missing var", () => {
  expect(() =>
    preflight(["TRIAGE_A", "TRIAGE_SET", "TRIAGE_B"], vars(["TRIAGE_SET", "1"])),
  ).toThrow("missing required environment variables: TRIAGE_A, TRIAGE_B");
  expect(() => preflight(["TRIAGE_A"], vars(["TRIAGE_A", ""]))).toThrow(ConfigError);
  expect(() => preflight(["TRIAGE_A"], vars(["TRIAGE_A", "1"]))).not.toThrow();
});

test("entries name every var in env form", () => {
  const config = loadConfig(vars(["TRIAGE_OTEL_ENDPOINT", "http://localhost:4317"]));
  expect(configEntries(config)).toEqual([
    ["TRIAGE_PORT", "7311"],
    ["TRIAGE_LOG_LEVEL", "info"],
    ["TRIAGE_LOG_FILE_LEVEL", "debug"],
    ["TRIAGE_LOG_FORMAT", "columns"],
    ["TRIAGE_OTEL_ENDPOINT", "http://localhost:4317/"],
  ]);
});

test("entries redact endpoint credentials and name the source var", () => {
  const config = loadConfig(
    vars(["OTEL_EXPORTER_OTLP_ENDPOINT", "http://user:hunter2@host:4317/v1?x=1"]),
  );
  expect(configEntries(config).at(-1)).toEqual([
    "TRIAGE_OTEL_ENDPOINT",
    "http://redacted:redacted@host:4317/v1?x=1 (from OTEL_EXPORTER_OTLP_ENDPOINT)",
  ]);
  expect(redactUserinfo(new URL("http://user@host"))).toBe("http://redacted@host/");
});

test("secret prints redacted in every string form", () => {
  const value = secret("hunter2");
  for (const shown of [
    String(value),
    `${value}`,
    JSON.stringify({ value }),
    Bun.inspect(value),
    Bun.inspect({ value }),
  ]) {
    expect(shown).toContain("[redacted]");
    expect(shown).not.toContain("hunter2");
  }
  expect(value.expose()).toBe("hunter2");
});

test("pathEnv drops empty values", () => {
  expect(pathEnv(vars(["HOME", "/h"], ["TRIAGE_DATA_DIR", ""], ["XDG_DATA_HOME", "/x"]))).toEqual({
    home: "/h",
    xdgDataHome: "/x",
  });
});
