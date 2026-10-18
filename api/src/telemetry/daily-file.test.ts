import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { logFile, openDailyFile } from "./daily-file";

const temps: string[] = [];

afterEach(() => {
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "triage-logs-"));
  temps.push(dir);
  return dir;
}

const fail = (err: unknown) => {
  throw err;
};

test("the file name pads the UTC date", () => {
  expect(logFile("/l", new Date("2026-03-05T23:30:00-05:00"))).toBe("/l/triage.2026-03-06.jsonl");
});

test("opening creates today's file", () => {
  const dir = tempDir();
  const now = new Date();
  openDailyFile(dir, fail, () => now).close();
  expect(readdirSync(dir)).toEqual([`triage.${now.toISOString().slice(0, 10)}.jsonl`]);
});

test("the first write after UTC midnight goes to the new day's file", () => {
  const dir = tempDir();
  let now = new Date("2026-10-18T23:59:59.999Z");
  const file = openDailyFile(dir, fail, () => now);
  file.write("one\n");
  now = new Date("2026-10-19T00:00:00.000Z");
  file.write("two\n");
  file.close();
  expect(readFileSync(join(dir, "triage.2026-10-18.jsonl"), "utf8")).toBe("one\n");
  expect(readFileSync(join(dir, "triage.2026-10-19.jsonl"), "utf8")).toBe("two\n");
});

test("seven files are kept, today's included", () => {
  const dir = tempDir();
  for (let day = 1; day <= 8; day += 1) {
    writeFileSync(join(dir, `triage.2020-01-0${day}.jsonl`), "");
  }
  writeFileSync(join(dir, "other.txt"), "");
  openDailyFile(dir, fail, () => new Date("2026-10-18T12:00:00Z")).close();
  expect(readdirSync(dir).sort()).toEqual([
    "other.txt",
    "triage.2020-01-03.jsonl",
    "triage.2020-01-04.jsonl",
    "triage.2020-01-05.jsonl",
    "triage.2020-01-06.jsonl",
    "triage.2020-01-07.jsonl",
    "triage.2020-01-08.jsonl",
    "triage.2026-10-18.jsonl",
  ]);
});

test("a missing dir or a file in its place fails to open", () => {
  const dir = tempDir();
  expect(() => openDailyFile(join(dir, "missing"), fail)).toThrow();
  writeFileSync(join(dir, "file"), "");
  expect(() => openDailyFile(join(dir, "file"), fail)).toThrow();
});

test("a failed rotation reports once, never throws, and retries on the next write", () => {
  const dir = tempDir();
  const logs = join(dir, "logs");
  mkdirSync(logs);
  const errors: unknown[] = [];
  let now = new Date("2026-10-18T23:59:59.999Z");
  const file = openDailyFile(
    logs,
    (err) => errors.push(err),
    () => now,
  );
  rmSync(logs, { recursive: true });
  now = new Date("2026-10-19T00:00:01.000Z");
  file.write("lost\n");
  file.write("lost\n");
  expect(errors).toHaveLength(1);
  mkdirSync(logs);
  file.write("kept\n");
  file.close();
  expect(readFileSync(join(logs, "triage.2026-10-19.jsonl"), "utf8")).toBe("kept\n");
});
