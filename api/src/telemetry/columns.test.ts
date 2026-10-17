import { expect, test } from "bun:test";
import { secret } from "../config";
import type { LogEvent } from "../telemetry";
import { causeChain, columnsLine, humanDuration, oneLine } from "./columns";

// Local components, so the clock column reads the same in every time zone.
const AT = new Date(2026, 0, 1, 12, 34, 56, 789);

function render(event: Partial<LogEvent> & Pick<LogEvent, "target">, ansi = false): string {
  return columnsLine({ time: AT, level: "info", message: "", fields: [], ...event }, ansi);
}

test("plain event", () => {
  const out = render({
    target: "triage::config",
    message: "loaded",
    fields: [
      ["count", 3],
      ["path", "/tmp"],
    ],
  });
  expect(out).toBe("12:34:56.789 INFO  config    loaded  count=3 path=/tmp\n");
});

test("an event in a request span ends with req", () => {
  const out = render({ target: "triage::http", level: "warn", message: "slow", requestId: "r1" });
  expect(out).toBe("12:34:56.789 WARN  http      slow  req=r1\n");
});

test("a closing span prints its fields and duration", () => {
  const out = render({
    target: "triage::http::middleware",
    message: "request",
    fields: [
      ["method", "GET"],
      ["status", 200],
    ],
    requestId: "r3",
    elapsedMs: 1.25,
  });
  expect(out).toBe(
    "12:34:56.789 INFO  middleware  request  method=GET status=200  1.3ms  req=r3\n",
  );
});

test("an error chain prints one line per cause", () => {
  const err = new Error("save failed", {
    cause: new Error("disk full", { cause: new Error("quota exceeded") }),
  });
  const out = render({
    target: "triage::store",
    level: "error",
    message: "write",
    fields: [["error", err]],
  });
  expect(out).toBe(
    `12:34:56.789 ERROR store     write  error="save failed"
                             └ disk full
                             └ quota exceeded
`,
  );
});

test("cause lines align under a long module", () => {
  const err = new Error("save failed", { cause: new Error("disk full") });
  const out = render({
    target: "triage::middleware",
    level: "error",
    message: "write",
    fields: [["error", err]],
  });
  expect(out).toBe(
    `12:34:56.789 ERROR middleware  write  error="save failed"
                               └ disk full
`,
  );
});

test("a span close line prints no cause lines", () => {
  const err = new Error("top", { cause: new Error("below") });
  const out = render({ target: "triage::x", fields: [["error", err]], elapsedMs: 2 });
  expect(out.split("\n")).toHaveLength(2);
});

test("newlines in values become markers", () => {
  const out = render({
    target: "triage::cli",
    message: "two\nlines",
    fields: [["body", "a\nb\r\nc"]],
  });
  expect(out).toBe("12:34:56.789 INFO  cli       two⏎lines  body=a⏎b⏎c\n");
});

test("control characters are escaped", () => {
  const out = render(
    { target: "triage::mail", message: "got", fields: [["subject", "\x1b[2J\x07red\rfake\tx"]] },
    true,
  );
  expect(out).toEndWith("subject=\\x1b[2J\\x07red⏎fake\\x09x\n");
  // Only the level colour's own two escapes.
  expect(out.split("\x1b")).toHaveLength(3);
});

test("values with spaces, equals, or quotes are quoted", () => {
  const out = render({
    target: "triage::x",
    message: "m",
    fields: [
      ["a", "x req=evil"],
      ["b", "k=v"],
      ["c", 'say "hi"'],
      ["d", "plain"],
    ],
    requestId: "a b",
  });
  expect(out).toBe(
    `12:34:56.789 INFO  x         m  a="x req=evil" b="k=v" c="say \\"hi\\"" d=plain  req="a b"\n`,
  );
});

test("objects print as JSON and a Secret prints bare", () => {
  const out = render({
    target: "triage::x",
    fields: [
      ["paths", { data: "/d" }],
      ["key", secret("hunter2")],
    ],
  });
  expect(out).toBe(`12:34:56.789 INFO  x         paths="{\\"data\\":\\"/d\\"}" key=[redacted]\n`);
});

test("an event without a message has a single gap", () => {
  expect(render({ target: "triage::x", fields: [["count", 1]] })).toBe(
    "12:34:56.789 INFO  x         count=1\n",
  );
});

test("ansi only when enabled", () => {
  expect(render({ target: "triage::x" })).not.toContain("\x1b");
  expect(render({ target: "triage::x" }, true)).toContain("\x1b[32mINFO \x1b[0m");
});

test("durations pick a readable unit", () => {
  expect(humanDuration(0.25)).toBe("250µs");
  expect(humanDuration(12.345)).toBe("12.3ms");
  expect(humanDuration(999.96)).toBe("1.00s");
  expect(humanDuration(2500)).toBe("2.50s");
});

test("a cyclic cause stops at the first repeat", () => {
  const top = new Error("top");
  const below = new Error("below", { cause: top });
  top.cause = below;
  expect(causeChain(top)).toEqual(["top", "below"]);
});

test("a non-Error cause ends the chain", () => {
  expect(causeChain(new Error("top", { cause: "reason" }))).toEqual(["top", "reason"]);
});

test("oneLine keeps printable text", () => {
  expect(oneLine("naïve ⏎ ok")).toBe("naïve ⏎ ok");
  expect(oneLine("a\u0085b")).toBe("a\\x85b");
});
