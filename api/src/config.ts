import { z } from "zod";

type Env = Readonly<Record<string, string | undefined>>;

/** Env vars that must be set before startup. None yet. */
const REQUIRED: readonly string[] = [];

const OTEL_ENDPOINT_VAR = "TRIAGE_OTEL_ENDPOINT";
const OTEL_TRACES_VAR = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
const OTEL_STANDARD_VAR = "OTEL_EXPORTER_OTLP_ENDPOINT";
const REDACTED = "[redacted]";
// Computed, because the naming rule rejects the literal key `toJSON`.
const TO_JSON = "toJSON";

/** A startup failure: 2 for bad configuration, 1 for an unusable filesystem. */
export class ConfigError extends Error {
  readonly exitCode: 1 | 2;

  constructor(message: string, exitCode: 1 | 2, options?: ErrorOptions) {
    super(message, options);
    this.name = "ConfigError";
    this.exitCode = exitCode;
  }
}

const PORT_RANGE = "expected a port from 1 to 65535";
const LOG_LEVELS = ["trace", "debug", "info", "warn", "error"] as const;

// `VAR=` means unset, as it does for preflight and TRIAGE_DATA_DIR.
const unsetWhenEmpty = (value: unknown) => (value === "" ? undefined : value);

// Messages never echo the value, which could be a misplaced secret.
const logLevel = (fallback: (typeof LOG_LEVELS)[number]) =>
  z.preprocess(
    unsetWhenEmpty,
    z
      .string()
      .transform((value) => value.toLowerCase())
      .pipe(z.enum(LOG_LEVELS, { error: `expected one of ${LOG_LEVELS.join(", ")}` }))
      .default(fallback),
  );

const envSchema = z.object({
  port: z.preprocess(
    unsetWhenEmpty,
    z
      .string()
      // Rust's integer parse also takes a leading `+` and figment trims the value.
      .regex(/^\s*\+?\d+\s*$/, PORT_RANGE)
      .transform(Number)
      .pipe(z.number().min(1, PORT_RANGE).max(65_535, PORT_RANGE))
      .default(7311),
  ),
  logLevel: logLevel("info"),
  // Rust's default `info,triage=debug` meant debug for our own code, and nothing else logs.
  logFileLevel: logLevel("debug"),
  logFormat: z.preprocess(
    unsetWhenEmpty,
    z.enum(["columns", "json"], { error: "use columns or json" }).default("columns"),
  ),
});

const SCHEMA_VARS = {
  port: "TRIAGE_PORT",
  logLevel: "TRIAGE_LOG_LEVEL",
  logFileLevel: "TRIAGE_LOG_FILE_LEVEL",
  logFormat: "TRIAGE_LOG_FORMAT",
} as const satisfies Record<keyof z.input<typeof envSchema>, string>;

/** Where OTLP traces go, and which env var set it. */
type OtelEndpoint = {
  readonly url: URL;
  readonly traces: URL;
  readonly var: string;
};

export type Config = z.output<typeof envSchema> & {
  readonly otelEndpoint?: OtelEndpoint;
  /** Bad settings that fell back instead of failing; telemetry logs them once it starts. */
  readonly warnings: readonly string[];
};

/** Checks {@link REQUIRED}, then parses `TRIAGE_*`. Bun has already read `.env` when running from source. */
export function loadConfig(env: Env = process.env): Config {
  preflight(REQUIRED, env);
  const parsed = envSchema.safeParse(
    Object.fromEntries(Object.entries(SCHEMA_VARS).map(([field, name]) => [field, env[name]])),
  );
  const otel = otelEndpoint(env);
  // A Set, because one bad value can fail several checks with the same message.
  const invalid = new Set([
    ...(parsed.success
      ? []
      : parsed.error.issues.map((issue) => `${varName(issue.path[0])}: ${issue.message}`)),
    ...(otel.error === undefined ? [] : [otel.error]),
  ]);
  if (!parsed.success || otel.error !== undefined) {
    throw new ConfigError(`invalid environment variables: ${[...invalid].join("; ")}`, 2);
  }
  return {
    ...parsed.data,
    ...(otel.endpoint === undefined ? {} : { otelEndpoint: otel.endpoint }),
    warnings: otel.warnings,
  };
}

function varName(field: PropertyKey | undefined): string {
  return Object.entries(SCHEMA_VARS).find(([key]) => key === field)?.[1] ?? String(field);
}

/** Names every absent var at once, where a parser would stop at the first. */
export function preflight(required: readonly string[], env: Env): void {
  const missing = required.filter((name) => !env[name]);
  if (missing.length > 0) {
    throw new ConfigError(`missing required environment variables: ${missing.join(", ")}`, 2);
  }
}

/** Each env var with its value as it would be set, `-` when unset, secrets redacted. */
export function configEntries(config: Config): [string, string][] {
  // `satisfies` fails to compile until a new field is listed here.
  const entries = {
    port: [SCHEMA_VARS.port, String(config.port)],
    logLevel: [SCHEMA_VARS.logLevel, config.logLevel],
    logFileLevel: [SCHEMA_VARS.logFileLevel, config.logFileLevel],
    logFormat: [SCHEMA_VARS.logFormat, config.logFormat],
    otelEndpoint: [
      OTEL_ENDPOINT_VAR,
      config.otelEndpoint === undefined ? "-" : formatEndpoint(config.otelEndpoint),
    ],
  } satisfies Record<Exclude<keyof Config, "warnings">, [string, string]>;
  return Object.values(entries);
}

function formatEndpoint(endpoint: OtelEndpoint): string {
  const url = redactUserinfo(endpoint.url);
  return endpoint.var === OTEL_ENDPOINT_VAR ? url : `${url} (from ${endpoint.var})`;
}

export function redactUserinfo(url: URL): string {
  const copy = new URL(url);
  if (copy.username !== "") {
    copy.username = "redacted";
  }
  if (copy.password !== "") {
    copy.password = "redacted";
  }
  return copy.href;
}

type OtelResult = {
  readonly endpoint?: OtelEndpoint;
  readonly error?: string;
  readonly warnings: readonly string[];
};

/**
 * `TRIAGE_OTEL_ENDPOINT` wins, and set but empty turns OTLP off; a bad value there exits 2. The
 * standard vars may be set shell-wide for other tools, so a bad one only warns and leaves OTLP off.
 */
function otelEndpoint(env: Env): OtelResult {
  const own = env[OTEL_ENDPOINT_VAR];
  if (own !== undefined) {
    if (own === "") {
      return { warnings: [] };
    }
    const parsed = parseEndpoint(OTEL_ENDPOINT_VAR, own);
    return typeof parsed === "string"
      ? { error: parsed, warnings: [] }
      : { endpoint: parsed, warnings: [] };
  }
  const name = [OTEL_TRACES_VAR, OTEL_STANDARD_VAR].find((candidate) => env[candidate]);
  const value = name === undefined ? undefined : env[name];
  if (name === undefined || value === undefined) {
    return { warnings: [] };
  }
  const parsed = parseEndpoint(name, value);
  return typeof parsed === "string"
    ? { warnings: [`${parsed}; OTLP export is off`] }
    : { endpoint: parsed, warnings: [] };
}

// Returns the error message, which never echoes the value: it may carry credentials.
function parseEndpoint(name: string, value: string): OtelEndpoint | string {
  const invalid = `${name}: expected an http URL with a host`;
  if (!URL.canParse(value)) {
    return invalid;
  }
  const url = new URL(value);
  // The exporter is built without TLS, so every https export would fail.
  if (url.protocol === "https:") {
    return `${name}: https is not supported; use an http collector`;
  }
  // `localhost:4317` parses as a URL with scheme `localhost`.
  if (url.protocol !== "http:" || url.hostname === "") {
    return invalid;
  }
  // The OTel spec takes the signal-specific var verbatim and appends the path to the base.
  const traces = name === OTEL_TRACES_VAR ? url : tracesUrl(url);
  return { url, traces, var: name };
}

export function tracesUrl(base: URL): URL {
  const dir = new URL(base);
  if (!dir.pathname.endsWith("/")) {
    dir.pathname = `${dir.pathname}/`;
  }
  return new URL("v1/traces", dir);
}

/** A value that prints `[redacted]` in every string form. */
export type Secret<T> = {
  readonly expose: () => T;
};

export function secret<T>(value: T): Secret<T> {
  return Object.freeze({
    expose: () => value,
    toString: () => REDACTED,
    [TO_JSON]: () => REDACTED,
    [Symbol.toPrimitive]: () => REDACTED,
    [Symbol.for("nodejs.util.inspect.custom")]: () => REDACTED,
  });
}

/** The env that `paths.ts` resolves dirs from, with empty values as unset. */
export type PathEnv = {
  readonly home?: string;
  readonly dataDir?: string;
  readonly xdgDataHome?: string;
  readonly xdgStateHome?: string;
  readonly xdgConfigHome?: string;
};

export function pathEnv(env: Env = process.env): PathEnv {
  const entries = {
    home: env.HOME,
    dataDir: env.TRIAGE_DATA_DIR,
    xdgDataHome: env.XDG_DATA_HOME,
    xdgStateHome: env.XDG_STATE_HOME,
    xdgConfigHome: env.XDG_CONFIG_HOME,
  };
  return Object.fromEntries(Object.entries(entries).filter(([, value]) => value));
}
