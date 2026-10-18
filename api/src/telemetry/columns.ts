import type { Level, LogEvent } from "../telemetry";

const MODULE_WIDTH = 8;
/** `HH:MM:SS.mmm LEVEL ` is 19 columns wide. */
const PREFIX_WIDTH = 19;
const COLOURS = {
  trace: 35,
  debug: 34,
  info: 32,
  warn: 33,
  error: 31,
} as const satisfies Record<Level, number>;

/**
 * `HH:MM:SS.mmm LEVEL module  message  key=value…  duration  req=<id>` in local time, then one
 * `└ ` line per cause of an error field. A span close line carries its duration and no causes.
 */
export function columnsLine(event: LogEvent, ansi: boolean): string {
  const label = event.level.toUpperCase().padEnd(5);
  const level = ansi ? `\x1b[${COLOURS[event.level]}m${label}\x1b[0m` : label;
  const module = event.target.split("::").at(-1) ?? "";
  const parts = [`${clock(event.time)} ${level} ${module.padEnd(MODULE_WIDTH)}`];
  const message = oneLine(event.message);
  if (message !== "") {
    parts.push(message);
  }
  if (event.fields.length > 0) {
    parts.push(
      event.fields.map(([key, value]) => `${key}=${quoted(oneLine(text(value)))}`).join(" "),
    );
  }
  if (event.elapsedMs !== undefined) {
    parts.push(humanDuration(event.elapsedMs));
  }
  if (event.requestId !== undefined) {
    parts.push(`req=${quoted(oneLine(event.requestId))}`);
  }
  const indent = " ".repeat(PREFIX_WIDTH + Math.max([...module].length, MODULE_WIDTH) + 2);
  const causes =
    event.elapsedMs === undefined
      ? event.fields.flatMap(([, value]) =>
          value instanceof Error ? causeChain(value).slice(1) : [],
        )
      : [];
  return [parts.join("  "), ...causes.map((cause) => `${indent}└ ${oneLine(cause)}`)]
    .map((line) => `${line}\n`)
    .join("");
}

function clock(time: Date): string {
  const pad = (value: number, width = 2) => String(value).padStart(width, "0");
  return `${pad(time.getHours())}:${pad(time.getMinutes())}:${pad(time.getSeconds())}.${pad(time.getMilliseconds(), 3)}`;
}

/** The messages of `err` and each `cause` below it, stopping at the first repeat. */
export function causeChain(err: Error): string[] {
  const chain: string[] = [];
  const seen = new Set<unknown>();
  let current: unknown = err;
  while (current !== undefined && current !== null && !seen.has(current)) {
    seen.add(current);
    if (!(current instanceof Error)) {
      chain.push(String(current));
      break;
    }
    chain.push(current.message === "" ? current.name : current.message);
    current = current.cause;
  }
  return chain;
}

/** A value as JSON text. Never throws: a bigint becomes a string, a cycle its `String()`. */
export function jsonText(value: unknown): string {
  if (value instanceof Error) {
    return JSON.stringify(causeChain(value).join(": "));
  }
  try {
    // The lib types say string, but a function or symbol gives undefined.
    const json: string | undefined = JSON.stringify(value, (_, inner: unknown) =>
      typeof inner === "bigint" ? String(inner) : inner,
    );
    return json ?? "null";
  } catch {
    return JSON.stringify(String(value));
  }
}

function text(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (value instanceof Error) {
    return causeChain(value)[0] ?? "";
  }
  if (typeof value === "object" && value !== null) {
    // A `toJSON` that returns a string, as `Secret` does, prints bare.
    const json = jsonText(value);
    const parsed: unknown = JSON.parse(json);
    return typeof parsed === "string" ? parsed : json;
  }
  return String(value);
}

/** Escapes control characters too, so a value cannot move the cursor or recolour a TTY. */
export function oneLine(value: string): string {
  return value.replace(/\r\n?|\n|\p{Cc}/gu, (match) =>
    match.startsWith("\r") || match === "\n"
      ? "⏎"
      : `\\x${(match.codePointAt(0) ?? 0).toString(16).padStart(2, "0")}`,
  );
}

// Unquoted, `a="x req=evil"` would read as a second field and spoof `req=`.
function quoted(value: string): string {
  return /[\s="]/u.test(value) ? JSON.stringify(value) : value;
}

export function humanDuration(ms: number): string {
  if (ms < 1) {
    return `${Math.floor(ms * 1000)}µs`;
  }
  // Below 999.95ms, so rounding never prints `1000.0ms`.
  if (ms < 999.95) {
    return `${ms.toFixed(1)}ms`;
  }
  return `${(ms / 1000).toFixed(2)}s`;
}
