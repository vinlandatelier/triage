import { AsyncLocalStorage } from "node:async_hooks";
import { isatty } from "node:tty";
import { type DestinationStream, type Logger, pino } from "pino";
import type { Config } from "./config";
import type { Paths } from "./paths";
import { causeChain, columnsLine, jsonText } from "./telemetry/columns";
import { type DailyFile, openDailyFile } from "./telemetry/daily-file";
import type { Otel, OtelAttributes, OtelSpan } from "./telemetry/otel";

export type Level = "trace" | "debug" | "info" | "warn" | "error";

/** Log fields. Keys are camelCase in code and snake_case on every sink: `requestId` is `request_id`. */
type Fields = Readonly<Record<string, unknown>>;

/** One event as every sink sees it, with field keys already snake_case. */
export type LogEvent = {
  readonly time: Date;
  readonly level: Level;
  readonly target: string;
  readonly message: string;
  readonly fields: readonly (readonly [string, unknown])[];
  /** The event's own `request_id`, else the nearest span's. */
  readonly requestId?: string;
  readonly span?: { readonly name: string; readonly id: string; readonly traceId: string };
  /** Set only on a span close line. */
  readonly elapsedMs?: number;
};

type SpanHandle = {
  /** Adds or replaces span fields, such as a status known only at the end. */
  readonly record: (fields: Fields) => void;
};

export type Log = {
  readonly [L in Level]: (message: string, fields?: Fields) => void;
} & {
  /** Runs `fn` inside a span. The only way to scope or time work; see `api-conventions`. */
  readonly withSpan: <T>(name: string, fields: Fields, fn: (span: SpanHandle) => T) => T;
};

export type Telemetry = {
  readonly logger: Logger;
  /** Flushes every sink and shuts OTLP down. Logging after it is a no-op. */
  readonly close: () => Promise<void>;
};

type SpanState = {
  readonly name: string;
  readonly target: string;
  readonly id: string;
  readonly traceId: string;
  readonly parent?: SpanState;
  readonly start: number;
  readonly fields: Map<string, unknown>;
  readonly otel?: OtelSpan;
};

type Active = {
  readonly root: Logger;
  /** Writes a span close line, when the stderr format and level show one. */
  readonly spanClose?: (event: LogEvent) => void;
  readonly otel?: Otel;
};

const EVENT = Symbol("triage.event");
const PINO_METADATA = Symbol.for("pino.metadata");
const RESERVED = new Set([
  "ts",
  "level",
  "target",
  "message",
  "span",
  "request_id",
  "span_id",
  "trace_id",
]);
// Matched against each `_` or `.` segment of a key, so `api_token` and `set_cookie` count too.
const SECRET_WORDS = new Set(["authorization", "cookie", "password", "secret", "token"]);
const LEVEL_VALUES = {
  trace: 10,
  debug: 20,
  info: 30,
  warn: 40,
  error: 50,
} as const satisfies Record<Level, number>;
const LEVELS = Object.keys(LEVEL_VALUES) as Level[];

const spans = new AsyncLocalStorage<SpanState>();
let nextSpanId = 0;
let active: Active | undefined;

type Options = {
  /** Replaces fd 2, for tests. */
  readonly stderr?: DestinationStream;
  readonly ansi?: boolean;
};

/**
 * Starts the stderr sink, the JSONL file under `paths.logs`, and OTLP when an endpoint is set. An
 * unusable logs dir logs one warning and leaves stderr only.
 */
export async function initTelemetry(
  config: Config,
  paths: Paths,
  options: Options = {},
): Promise<Telemetry> {
  const stderr = options.stderr ?? stderrDestination();
  const ansi = options.ansi ?? isatty(2);
  const renderStderr =
    config.logFormat === "columns" ? (event: LogEvent) => columnsLine(event, ansi) : jsonLine;
  const writeStderr = (event: LogEvent) => {
    if (LEVEL_VALUES[event.level] >= LEVEL_VALUES[config.logLevel]) {
      stderr.write(renderStderr(event));
    }
  };
  const warnings = [...config.warnings];
  const streams = [
    { level: config.logLevel, stream: sink(renderStderr, (line) => stderr.write(line)) },
  ];
  let file: DailyFile | undefined;
  try {
    // A failing file must not log through itself, so its one warning goes to stderr alone.
    file = openDailyFile(paths.logs, (err) =>
      writeStderr(
        buildEvent(
          "warn",
          "triage::telemetry",
          "cannot write the log file; file logging may stop",
          {
            dir: paths.logs,
            error: err,
          },
        ),
      ),
    );
    const open = file;
    streams.push({
      level: config.logFileLevel,
      stream: sink(jsonLine, (line) => open.write(line)),
    });
  } catch (err) {
    warnings.push(
      `cannot write log files in ${paths.logs}: ${describe(err)}; logging to stderr only`,
    );
  }

  const level = LEVELS.find((name) => streams.some((stream) => stream.level === name)) ?? "info";
  // `formatters.log` empties pino's own JSON: every sink renders from the event instead.
  const root = pino(
    { level, base: null, timestamp: false, formatters: { log: () => ({}) } },
    pino.multistream(streams),
  );
  const log = logger("telemetry");
  const otel =
    config.otelEndpoint === undefined
      ? undefined
      : (await import("./telemetry/otel")).startOtel(config.otelEndpoint.traces, log.warn);
  const state: Active = {
    root,
    ...(config.logFormat === "columns" ? { spanClose: writeStderr } : {}),
    ...(otel === undefined ? {} : { otel }),
  };
  active = state;
  for (const warning of warnings) {
    log.warn(warning);
  }

  let closed = false;
  return {
    logger: root,
    async close() {
      if (closed) {
        return;
      }
      closed = true;
      try {
        // First, so a failed final export still reaches the file.
        await otel?.shutdown();
      } finally {
        if (active === state) {
          active = undefined;
        }
        root.level = "silent";
        file?.close();
      }
    },
  };
}

function stderrDestination(): DestinationStream {
  const dest = pino.destination({ fd: 2, sync: true });
  // A closed stderr has nowhere left to report to, and an unhandled `error` event would crash.
  dest.on("error", () => undefined);
  return dest;
}

/** A pino destination that renders the event a log call carried, ignoring pino's own line. */
function sink(render: (event: LogEvent) => string, write: (line: string) => void) {
  return {
    [PINO_METADATA]: true,
    lastObj: undefined as unknown,
    write() {
      const carried: unknown =
        typeof this.lastObj === "object" && this.lastObj !== null
          ? (this.lastObj as Record<symbol, unknown>)[EVENT]
          : undefined;
      if (carried !== undefined) {
        write(render(carried as LogEvent));
      }
    },
  };
}

/** A logger for one module. Call it once at module level: `const log = logger("store")`. */
export function logger(module: string): Log {
  const target = `triage::${module}`;
  const emit =
    (level: Level) =>
    (message: string, fields: Fields = {}) => {
      active?.root[level]({
        [EVENT]: buildEvent(level, target, message, fields, spans.getStore()),
      });
    };
  return {
    trace: emit("trace"),
    debug: emit("debug"),
    info: emit("info"),
    warn: emit("warn"),
    error: emit("error"),
    withSpan: (name, fields, fn) => withSpan(target, name, fields, fn),
  };
}

function buildEvent(
  level: Level,
  target: string,
  message: string,
  fields: Fields,
  span?: SpanState,
): LogEvent {
  const entries = normalize(fields);
  const own = entries.find(([key]) => key === "request_id");
  const requestId = own === undefined ? spanRequestId(span) : String(own[1]);
  return {
    time: new Date(),
    level,
    target,
    message,
    fields: entries.filter(([key]) => key !== "request_id"),
    ...(requestId === undefined ? {} : { requestId }),
    ...(span === undefined
      ? {}
      : { span: { name: span.name, id: span.id, traceId: span.traceId } }),
  };
}

function normalize(fields: Fields): [string, unknown][] {
  return Object.entries(fields)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => {
      const name = key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
      const secret = name.split(/[_.]/).some((word) => SECRET_WORDS.has(word));
      return [name, secret ? "[redacted]" : value];
    });
}

function spanRequestId(span: SpanState | undefined): string | undefined {
  for (let current = span; current !== undefined; current = current.parent) {
    const id = current.fields.get("request_id");
    if (id !== undefined) {
      return String(id);
    }
  }
  return undefined;
}

function withSpan<T>(target: string, name: string, fields: Fields, fn: (span: SpanHandle) => T): T {
  const parent = spans.getStore();
  nextSpanId += 1;
  const id = String(nextSpanId);
  const otel = active?.otel;
  const state: SpanState = {
    name,
    target,
    id,
    traceId: parent?.traceId ?? id,
    ...(parent === undefined ? {} : { parent }),
    start: performance.now(),
    fields: new Map(normalize(fields)),
    ...(otel === undefined ? {} : { otel: otel.startSpan(name, attributes(fields), parent?.otel) }),
  };
  const handle: SpanHandle = {
    record(more) {
      for (const [key, value] of normalize(more)) {
        state.fields.set(key, value);
      }
      if (state.otel !== undefined) {
        otel?.record(state.otel, attributes(more));
      }
    },
  };
  let result: T;
  try {
    result = spans.run(state, () => fn(handle));
  } catch (err) {
    closeSpan(state, err);
    throw err;
  }
  if (result instanceof Promise) {
    return result.then(
      (value: unknown) => {
        closeSpan(state);
        return value;
      },
      (err: unknown) => {
        closeSpan(state, err);
        throw err;
      },
    ) as T;
  }
  closeSpan(state);
  return result;
}

function closeSpan(state: SpanState, error?: unknown): void {
  if (state.otel !== undefined) {
    active?.otel?.endSpan(state.otel, error);
  }
  const fields = [...state.fields].filter(([key]) => key !== "request_id");
  const requestId = spanRequestId(state);
  active?.spanClose?.({
    time: new Date(),
    level: "info",
    target: state.target,
    message: state.name,
    fields,
    ...(requestId === undefined ? {} : { requestId }),
    elapsedMs: performance.now() - state.start,
  });
}

function attributes(fields: Fields): OtelAttributes {
  return Object.fromEntries(
    normalize(fields).map(([key, value]) => [
      key,
      typeof value === "string" || typeof value === "number" || typeof value === "boolean"
        ? value
        : value instanceof Error
          ? causeChain(value).join(": ")
          : jsonText(value),
    ]),
  );
}

/**
 * `ts level target message`, the event's fields, then `span request_id span_id trace_id`. A field
 * named like one of those keys moves to `field.<name>`, prefixed again until free.
 */
function jsonLine(event: LogEvent): string {
  const taken = new Set(event.fields.map(([key]) => key));
  const freeKey = (key: string) => {
    if (!RESERVED.has(key)) {
      return key;
    }
    let free = `field.${key}`;
    while (taken.has(free)) {
      free = `field.${free}`;
    }
    taken.add(free);
    return free;
  };
  // Built by hand: an object would move integer-like keys ahead of `ts`.
  const entries: [string, unknown][] = [
    ["ts", event.time.toISOString()],
    ["level", event.level.toUpperCase()],
    ["target", event.target],
    ["message", event.message],
    ...event.fields.map(([key, value]): [string, unknown] => [freeKey(key), value]),
  ];
  if (event.span !== undefined) {
    entries.push(["span", event.span.name]);
  }
  if (event.requestId !== undefined) {
    entries.push(["request_id", event.requestId]);
  }
  if (event.span !== undefined) {
    entries.push(["span_id", event.span.id], ["trace_id", event.span.traceId]);
  }
  return `{${entries.map(([key, value]) => `${JSON.stringify(key)}:${jsonText(value)}`).join(",")}}\n`;
}

function describe(err: unknown): string {
  return err instanceof Error ? causeChain(err).join(": ") : String(err);
}
