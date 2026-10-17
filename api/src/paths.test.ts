import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ConfigError } from "./config";
import { createPaths, resolvePaths } from "./paths";

const temps: string[] = [];

function tempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "triage-paths-"));
  temps.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of temps.splice(0)) {
    chmodSync(dir, 0o700);
    rmSync(dir, { recursive: true, force: true });
  }
});

function mode(path: string): number {
  return statSync(path).mode & 0o777;
}

test("macOS uses Application Support and Library/Logs", () => {
  expect(resolvePaths(undefined, { home: "/Users/u" }, "darwin")).toEqual({
    data: "/Users/u/Library/Application Support/studio.triage",
    logs: "/Users/u/Library/Logs/triage",
    config: "/Users/u/Library/Application Support/studio.triage",
  });
});

test("Linux uses absolute XDG vars and falls back for unset or relative ones", () => {
  expect(
    resolvePaths(
      undefined,
      { home: "/home/u", xdgDataHome: "/xd", xdgStateHome: "rel", xdgConfigHome: "/xc" },
      "linux",
    ),
  ).toEqual({
    data: "/xd/triage",
    logs: "/home/u/.local/state/triage/logs",
    config: "/xc/triage",
  });
  expect(resolvePaths(undefined, { home: "/home/u" }, "linux")).toEqual({
    data: "/home/u/.local/share/triage",
    logs: "/home/u/.local/state/triage/logs",
    config: "/home/u/.config/triage",
  });
});

test.each([undefined, "relative"])("HOME=%s without a data dir exits 1", (home) => {
  const env = home === undefined ? {} : { home };
  expect(() => resolvePaths(undefined, env, "linux")).toThrow(
    new ConfigError("cannot find a home directory; pass --data-dir <path>", 1),
  );
});

test("the flag wins over TRIAGE_DATA_DIR, which wins over the platform", () => {
  expect(resolvePaths("/flag", { dataDir: "/env", home: "/h" }).data).toBe("/flag");
  expect(resolvePaths(undefined, { dataDir: "/env" }).data).toBe("/env");
});

test("a data dir holds logs and config", () => {
  expect(resolvePaths("/d", {})).toEqual({ data: "/d", logs: "/d/logs", config: "/d/config" });
});

test.each(["~/x", "~user/x"])("data dir %s exits 2", (dir) => {
  let caught: unknown;
  try {
    resolvePaths(dir, {});
  } catch (err) {
    caught = err;
  }
  expect(caught).toBeInstanceOf(ConfigError);
  expect((caught as ConfigError).exitCode).toBe(2);
});

test("a relative data dir becomes absolute like Rust's std::path::absolute", () => {
  const cwd = process.cwd();
  expect(resolvePaths("rel", {}).data).toBe(`${cwd}/rel`);
  expect(resolvePaths("./a/./b", {}).data).toBe(`${cwd}/a/b`);
  expect(resolvePaths("//x//y", {}).data).toBe("//x/y");
  expect(resolvePaths("///x", {}).data).toBe("/x");
  expect(resolvePaths("a/../b/", {})).toEqual({
    data: `${cwd}/a/../b/`,
    logs: `${cwd}/a/../b/logs`,
    config: `${cwd}/a/../b/config`,
  });
});

test("createPaths makes every dir 0700", () => {
  const root = join(tempDir(), "t");
  const paths = resolvePaths(root, {});
  createPaths(paths);
  for (const dir of [paths.data, paths.logs, paths.config]) {
    expect(mode(dir)).toBe(0o700);
  }
});

test("createPaths tightens existing open dirs to 0700", () => {
  const paths = resolvePaths(join(tempDir(), "t"), {});
  for (const dir of [paths.data, paths.config]) {
    mkdirSync(dir, { recursive: true });
    chmodSync(dir, 0o755);
  }
  createPaths(paths);
  expect(mode(paths.data)).toBe(0o700);
  expect(mode(paths.config)).toBe(0o700);
});

test("a read-only data dir exits 1 naming the path and the OS error", () => {
  const data = join(tempDir(), "ro");
  mkdirSync(data, { mode: 0o500 });
  try {
    writeFileSync(join(data, "root-check"), "");
    // Root ignores the mode bits, so there is nothing to test.
    return;
  } catch {
    // Expected for a normal user.
  }
  let caught: unknown;
  try {
    createPaths(resolvePaths(data, {}));
  } catch (err) {
    caught = err;
  }
  chmodSync(data, 0o700);
  expect(caught).toBeInstanceOf(ConfigError);
  const err = caught as ConfigError;
  expect(err.exitCode).toBe(1);
  expect(err.message).toContain(data);
  expect(err.message).toContain("permission denied");
});

test("a read-only logs dir is left to telemetry", () => {
  const paths = resolvePaths(join(tempDir(), "t"), {});
  mkdirSync(paths.data, { recursive: true });
  mkdirSync(paths.logs, { mode: 0o500 });
  expect(() => createPaths(paths)).not.toThrow();
  chmodSync(paths.logs, 0o700);
});
