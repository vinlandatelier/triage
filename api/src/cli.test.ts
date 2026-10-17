import { Database } from "bun:sqlite";
import { afterEach, expect, test } from "bun:test";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { CliError, configTable, rowsTable, schemaDrift, writeOut } from "./cli";
import { loadConfig } from "./config";

const MAIN = join(import.meta.dir, "main.ts");
const temps: string[] = [];

afterEach(() => {
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDir(): string {
  // realpath, because macOS tmpdir is a symlink and the binary prints its absolute cwd.
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "triage-cli-")));
  temps.push(dir);
  return dir;
}

type Run = { readonly exitCode: number; readonly stdout: string; readonly stderr: string };

function vars(...pairs: [string, string][]): Record<string, string> {
  return Object.fromEntries(pairs);
}

// From a temp cwd with only HOME set, so no `.env` or caller TRIAGE_* or OTEL_* var leaks in.
async function triage(
  args: readonly string[],
  options: { readonly cwd?: string; readonly env?: Record<string, string> } = {},
): Promise<Run> {
  const proc = Bun.spawn([process.execPath, MAIN, ...args], {
    cwd: options.cwd ?? tempDir(),
    env: { ...vars(["HOME", tmpdir()]), ...options.env },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { exitCode, stdout, stderr };
}

test("config show prints aligned vars and paths and creates nothing", async () => {
  const data = join(tempDir(), "fresh");
  const run = await triage(["config", "show", "--data-dir", data]);
  expect(run).toEqual({
    exitCode: 0,
    stderr: "",
    stdout: `TRIAGE_PORT            7311
TRIAGE_LOG_LEVEL       info
TRIAGE_LOG_FILE_LEVEL  debug
TRIAGE_LOG_FORMAT      columns
TRIAGE_OTEL_ENDPOINT   -
data_dir               ${data}
logs_dir               ${data}/logs
config_dir             ${data}/config
`,
  });
  expect(existsSync(data)).toBe(false);
});

test("--data-dir before the subcommand works too and wins over TRIAGE_DATA_DIR", async () => {
  const run = await triage(["--data-dir", "/flag", "config", "show"], {
    env: vars(["TRIAGE_DATA_DIR", "/env"]),
  });
  expect(run.stdout).toContain("data_dir               /flag\n");
});

test("a relative --data-dir resolves under the cwd", async () => {
  const cwd = tempDir();
  const run = await triage(["config", "show", "--data-dir", "./tmp"], { cwd });
  expect(run.exitCode).toBe(0);
  expect(run.stdout).toContain(`data_dir               ${cwd}/tmp\n`);
  expect(run.stdout).toContain(`logs_dir               ${cwd}/tmp/logs\n`);
  expect(run.stdout).toContain(`config_dir             ${cwd}/tmp/config\n`);
});

test("a --data-dir starting with ~ exits 2", async () => {
  const run = await triage(["config", "show", "--data-dir", "~/x"]);
  expect(run.exitCode).toBe(2);
  expect(run.stderr).toContain("`~` is not expanded");
});

test("no absolute HOME and no data dir exits 1", async () => {
  const run = await triage(["config", "show"], { env: vars(["HOME", "relative"]) });
  expect(run.exitCode).toBe(1);
  expect(run.stderr).toBe("error: cannot find a home directory; pass --data-dir <path>\n");
});

test("TRIAGE_PORT=abc exits 2 naming the var", async () => {
  const run = await triage(["config", "show", "--data-dir", tempDir()], {
    env: vars(["TRIAGE_PORT", "abc"]),
  });
  expect(run).toEqual({
    exitCode: 2,
    stdout: "",
    stderr: "error: invalid environment variables: TRIAGE_PORT: expected a port from 1 to 65535\n",
  });
});

test("secrets in the OTLP endpoint print redacted", async () => {
  const run = await triage(["config", "show", "--data-dir", tempDir()], {
    env: vars(["OTEL_EXPORTER_OTLP_ENDPOINT", "http://user:hunter2@host:4317"]),
  });
  expect(run.stdout).toContain(
    "TRIAGE_OTEL_ENDPOINT   http://redacted:redacted@host:4317/ (from OTEL_EXPORTER_OTLP_ENDPOINT)\n",
  );
  expect(run.stdout).not.toContain("hunter2");
});

test.each([
  ["bash", "complete -F"],
  ["zsh", "#compdef triage"],
  ["fish", "complete -c triage"],
])("completions %s prints a script without loading config", async (shell, marker) => {
  const run = await triage(["completions", shell], {
    env: vars(["TRIAGE_PORT", "abc"], ["HOME", "relative"]),
  });
  expect(run.exitCode).toBe(0);
  expect(run.stderr).toBe("");
  expect(run.stdout).toContain(marker);
  if (shell === "zsh") {
    expect(run.stdout.startsWith("#compdef triage\n")).toBe(true);
  }
});

test.each([
  [["config", "show", "--data-dir"], "--data-dir needs a value"],
  [["config", "show", "--data-dir", ""], "--data-dir needs a value"],
  [["bogus"], "Unknown command bogus"],
  [["config"], "No command specified."],
])("%p is a usage error that exits 2", async (args, message) => {
  const run = await triage(args);
  expect(run.exitCode).toBe(2);
  expect(run.stdout).toBe("");
  expect(run.stderr).toContain(`error: ${message}\n`);
});

test("completions for an unknown shell exits 2", async () => {
  const run = await triage(["completions", "powershell"]);
  expect(run.exitCode).toBe(2);
  expect(run.stderr).toContain("use bash, zsh, fish");
});

test("the generated scripts complete subcommands", async () => {
  const run = await triage(["complete", "--", "con"]);
  expect(run.stdout).toContain("config\t");
});

test("logs path prints today's JSONL file, which already exists", async () => {
  const data = join(tempDir(), "fresh");
  const run = await triage(["logs", "path", "--data-dir", data]);
  const today = new Date().toISOString().slice(0, 10);
  expect(run).toEqual({
    exitCode: 0,
    stderr: "",
    stdout: `${data}/logs/triage.${today}.jsonl\n`,
  });
  expect(existsSync(run.stdout.trim())).toBe(true);
});

test("logs path with an unusable logs dir warns once and still prints the path", async () => {
  const data = tempDir();
  writeFileSync(join(data, "logs"), "");
  const run = await triage(["logs", "path", "--data-dir", data]);
  expect(run.exitCode).toBe(0);
  expect(run.stdout).toStartWith(`${data}/logs/triage.`);
  expect(run.stderr.split("\n").filter((line) => line !== "")).toHaveLength(1);
  expect(run.stderr).toContain(`cannot write log files in ${data}/logs`);
});

test("logs path with a bad level exits 2", async () => {
  const run = await triage(["logs", "path", "--data-dir", tempDir()], {
    env: vars(["TRIAGE_LOG_LEVEL", "nonsense"]),
  });
  expect(run.exitCode).toBe(2);
  expect(run.stderr).toContain("TRIAGE_LOG_LEVEL");
});

test("logs path logs the data dir override at debug", async () => {
  const data = tempDir();
  const run = await triage(["logs", "path", "--data-dir", data], {
    env: vars(["TRIAGE_DATA_DIR", "/elsewhere"], ["TRIAGE_LOG_LEVEL", "debug"]),
  });
  expect(run.stderr).toContain("--data-dir overrides TRIAGE_DATA_DIR");
  expect(run.stderr).toContain("startup");
});

test("configTable aligns values after the longest key", () => {
  const table = configTable(loadConfig({}), { data: "/d", logs: "/d/logs", config: "/d/config" });
  expect(table.split("\n").at(-2)).toBe("config_dir             /d/config");
});

test("a closed stdout is success and any other write error exits 1", async () => {
  const failing = (code: string) => () => {
    const err = Object.assign(new Error(code), { code });
    return Promise.reject(err);
  };
  await writeOut(failing("EPIPE"), "x");
  const err = await writeOut(failing("ENOSPC"), "x").catch((caught: unknown) => caught);
  expect(err).toBeInstanceOf(CliError);
  expect((err as CliError).exitCode).toBe(1);
});

function db(dir: string, ...args: string[]): Promise<Run> {
  return triage(["db", ...args, "--data-dir", dir]);
}

test("db migrate applies once, then check passes", async () => {
  const dir = tempDir();
  const first = await db(dir, "migrate");
  expect(first.exitCode).toBe(0);
  expect(first.stdout).toMatch(/^applied \d+ migrations?\n$/);
  expect((await db(dir, "migrate")).stdout).toBe("applied 0 migrations\n");
  expect(existsSync(join(dir, "triage.db-wal"))).toBe(false);
  expect(await db(dir, "check")).toEqual({
    exitCode: 0,
    stdout: "schema matches schema.sql\n",
    stderr: "",
  });
});

test("db schema prints schema.sql", async () => {
  const run = await db(tempDir(), "schema");
  expect(run.exitCode).toBe(0);
  expect(run.stdout).toBe(readFileSync(join(import.meta.dir, "../schema.sql"), "utf8"));
});

test("db check fails on drift", async () => {
  const dir = tempDir();
  await db(dir, "migrate");
  const client = new Database(join(dir, "triage.db"));
  client.run("CREATE TABLE stray (x INTEGER)");
  client.close();
  const run = await db(dir, "check");
  expect(run.exitCode).toBe(1);
  expect(run.stdout).toContain("schema drift at line");
});

test.each([[["check"]], [["sql", "SELECT 1"]]])(
  "db %p needs an existing database and creates none",
  async (args) => {
    const dir = tempDir();
    const run = await db(dir, ...args);
    expect(run.exitCode).toBe(1);
    expect(run.stderr).toContain("run `triage db migrate`");
    expect(existsSync(join(dir, "triage.db"))).toBe(false);
  },
);

test("db sql rejects writes", async () => {
  const dir = tempDir();
  await db(dir, "migrate");
  const run = await db(dir, "sql", "WITH x AS (SELECT 1) DELETE FROM _migrations");
  expect(run.exitCode).toBe(1);
  expect(run.stderr).toContain("readonly database");
});

test("db sql rejects more than one statement before running it", async () => {
  const dir = tempDir();
  await db(dir, "migrate");
  const run = await db(dir, "sql", "SELECT 1 AS a, 2 AS b; SELECT 3");
  expect(run).toEqual({
    exitCode: 1,
    stdout: "",
    stderr: "error: pass exactly one statement\n",
  });
});

test("db sql prints a table or json", async () => {
  const dir = tempDir();
  await db(dir, "migrate");
  const query = "SELECT 1 AS n, 'x' AS t";
  expect((await db(dir, "sql", query)).stdout).toBe("n  t\n-  -\n1  x\n");
  expect((await db(dir, "sql", "--json", query)).stdout).toBe(
    '[\n  {\n    "n": 1,\n    "t": "x"\n  }\n]\n',
  );
  expect((await db(dir, "sql", "-- no rows\nSELECT 1 AS n WHERE 0")).stdout).toBe("n\n-\n");
  expect(
    (await triage(["db", "sql", "--json", "--data-dir", dir, "-- c\nSELECT 2 AS n"])).stdout,
  ).toBe('[\n  {\n    "n": 2\n  }\n]\n');
  expect((await db(dir, "sql", "SELECT x'00ff', 9007199254740993")).stdout).toBe(
    "x'00ff'  9007199254740993\n-------  ----------------\nx'00ff'  9007199254740993\n",
  );
});

test("rowsTable pads columns and marks nulls and newlines", () => {
  expect(
    rowsTable({
      columns: ["id", "no\nte"],
      rows: [
        [1n, "a\nb\r\nc"],
        [200n, null],
        [3n, "\t\u001b[31m"],
        [4n, "é"],
      ],
    }),
  ).toBe(`id   no⏎te
---  ------------
1    a⏎b⏎c
200  NULL
3    \\x09\\x1b[31m
4    é
`);
});

test("schemaDrift names the first differing line", () => {
  expect(schemaDrift("a\nb\n", "a\nb\n")).toBeUndefined();
  expect(schemaDrift("a\nb\n", "a\nc\nd\n")).toBe(
    "schema drift at line 2\n  schema.sql  b\n  database    c\n",
  );
  expect(schemaDrift("a\n", "a\nb\n")).toBe(
    "schema drift at line 2\n  schema.sql  (end)\n  database    b\n",
  );
  expect(schemaDrift("a\n", "a\n\nb\n")).toBe(
    "schema drift at line 2\n  schema.sql  (end)\n  database    (blank line)\n",
  );
});
