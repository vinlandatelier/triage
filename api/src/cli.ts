import { script } from "@bomb.sh/tab";
import tab from "@bomb.sh/tab/citty";
import { type ArgsDef, defineCommand, parseArgs, renderUsage, runCommand, runMain } from "citty";
import packageJson from "../package.json" with { type: "json" };
import { type Config, configEntries, loadConfig, pathEnv } from "./config";
import { probe } from "./http/probe";
import { type Lock, lockDataDir, serve, shutdownSignals } from "./http/serve";
import { createPaths, dbFile, type Paths, resolvePaths } from "./paths";
import { openReadOnly, openStore } from "./store/open";
import { cellText, type Rows, rowsJson, runQuery } from "./store/query";
import { dumpSchema, SCHEMA } from "./store/schema";
import { initTelemetry, logger, type Telemetry } from "./telemetry";
import { oneLine } from "./telemetry/columns";
import { logFile } from "./telemetry/daily-file";

/** A command failure with its exit code. */
export class CliError extends Error {
  readonly exitCode: 1 | 2;

  constructor(message: string, exitCode: 1 | 2, options?: ErrorOptions) {
    super(message, options);
    this.name = "CliError";
    this.exitCode = exitCode;
  }
}

const log = logger("cli");

const SHELLS = ["bash", "zsh", "fish"] as const;
type Shell = (typeof SHELLS)[number];

// Declared on every command so help lists it; read once from the whole argv, because citty
// hands a subcommand only the args after its name.
const globalArgs = {
  "data-dir": {
    type: "string",
    valueHint: "PATH",
    description: "Keep data, logs, and config under PATH. Overrides TRIAGE_DATA_DIR",
  },
} satisfies ArgsDef;

export async function run(argv: string[]): Promise<void> {
  const rawArgs = hyphenQuery(argv);
  const dataDir = parseArgs(rawArgs, globalArgs)["data-dir"];
  // An empty dir would resolve to the cwd and scatter data wherever the command ran.
  if (dataDir === "") {
    throw new CliError("--data-dir needs a value", 2);
  }
  const root = await rootCommand(dataDir);
  const builtin =
    rawArgs.some((arg) => arg === "--help" || arg === "-h") ||
    (rawArgs.length === 1 && (rawArgs[0] === "--version" || rawArgs[0] === "-v"));
  if (builtin) {
    await runMain(root, { rawArgs });
    return;
  }
  try {
    await runCommand(root, { rawArgs });
  } catch (err) {
    if (!(err instanceof Error && err.name === "CLIError")) {
      throw err;
    }
    // An unknown or missing command fails before any command runs, so runMain can dispatch
    // again to find the right subcommand's usage. Throwing from `showUsage` skips its exit 1.
    await runMain(root, {
      rawArgs,
      async showUsage(cmd, parent) {
        console.error(await renderUsage(cmd, parent));
        throw new CliError(Bun.stripANSI(err.message), 2, { cause: err });
      },
    });
  }
}

const SQL_FLAGS = new Set(["--json", "--help", "-h"]);

// clap's `allow_hyphen_values` on `db sql <QUERY>`: a query that starts with `-`, such as a
// leading `--` comment, is the query, not a flag. citty reads everything after `--` as positional.
function hyphenQuery(argv: readonly string[]): string[] {
  const sql = argv.indexOf("sql");
  if (sql < 1 || !argv.slice(0, sql).includes("db") || argv.includes("--")) {
    return [...argv];
  }
  for (let i = sql + 1; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    if (arg === "--data-dir") {
      i++;
    } else if (!(arg.startsWith("--data-dir=") || SQL_FLAGS.has(arg))) {
      return arg.startsWith("-")
        ? [...argv.slice(0, i), ...argv.slice(i + 1), "--", arg]
        : [...argv];
    }
  }
  return [...argv];
}

async function rootCommand(dataDir: string | undefined) {
  const root = defineCommand({
    meta: {
      name: "triage",
      version: packageJson.version,
      description: "Local life-admin agent",
    },
    args: globalArgs,
    subCommands: {
      serve: defineCommand({
        meta: {
          name: "serve",
          description: "Run the daemon in the foreground until SIGINT or SIGTERM",
        },
        args: globalArgs,
        async run() {
          await serveCommand(dataDir);
        },
      }),
      status: defineCommand({
        meta: {
          name: "status",
          description: "Report whether the daemon is running; exits 3 when not",
        },
        args: globalArgs,
        async run() {
          await withPaths(dataDir, async (_paths, config) => {
            const health = await probe(config.port);
            if (health === undefined) {
              await emit("not running\n");
              // LSB `status` code for "not running", so scripts can tell it from a failed check.
              process.exitCode = 3;
              return;
            }
            await emit(
              `running v${health.version} on 127.0.0.1:${config.port}, up ${uptime(health.uptime_s)}\n`,
            );
          });
        },
      }),
      config: defineCommand({
        meta: { name: "config", description: "Inspect the resolved configuration" },
        args: globalArgs,
        subCommands: {
          show: defineCommand({
            meta: {
              name: "show",
              description: "Print every config value and path, secrets redacted",
            },
            args: globalArgs,
            async run() {
              // Read-only, so it never creates a dir.
              const paths = resolvePaths(dataDir, pathEnv());
              await emit(configTable(loadConfig(), paths));
            },
          }),
        },
      }),
      logs: defineCommand({
        meta: { name: "logs", description: "Locate log files" },
        args: globalArgs,
        subCommands: {
          path: defineCommand({
            meta: {
              name: "path",
              description: "Print the absolute path of today's JSONL log file",
            },
            args: globalArgs,
            async run() {
              const { paths, telemetry } = await start(dataDir);
              try {
                await emit(`${logFile(paths.logs, new Date())}\n`);
              } finally {
                await telemetry.close();
              }
            },
          }),
        },
      }),
      db: defineCommand({
        meta: { name: "db", description: "Inspect and migrate the database" },
        args: globalArgs,
        subCommands: {
          schema: defineCommand({
            meta: {
              name: "schema",
              description:
                "Migrate the database, then print its schema as ordered `CREATE` statements",
            },
            args: globalArgs,
            async run() {
              await withPaths(dataDir, async (paths) => {
                const { store } = openStore(dbFile(paths));
                let schema: string;
                try {
                  schema = dumpSchema(store.db.$client);
                } finally {
                  store.close();
                }
                await emit(schema);
              });
            },
          }),
          migrate: defineCommand({
            meta: {
              name: "migrate",
              description: "Apply pending migrations and print how many ran",
            },
            args: globalArgs,
            async run() {
              await withPaths(dataDir, async (paths) => {
                const { store, applied } = openStore(dbFile(paths));
                store.close();
                await emit(`applied ${applied} migration${applied === 1 ? "" : "s"}\n`);
              });
            },
          }),
          check: defineCommand({
            meta: {
              name: "check",
              description:
                "Compare the database schema to the `schema.sql` this build ships; exits 1 on drift",
            },
            args: globalArgs,
            async run() {
              await withPaths(dataDir, async (paths) => {
                const client = openReadOnly(dbFile(paths));
                let live: string;
                try {
                  live = dumpSchema(client);
                } finally {
                  client.close();
                }
                const drift = schemaDrift(SCHEMA, live);
                await emit(drift ?? "schema matches schema.sql\n");
                if (drift !== undefined) {
                  process.exitCode = 1;
                }
              });
            },
          }),
          sql: defineCommand({
            meta: {
              name: "sql",
              description: "Run one query on a read-only connection and print the rows",
            },
            args: {
              ...globalArgs,
              query: { type: "positional", required: true, description: "The SQL statement" },
              json: {
                type: "boolean",
                description: "Print a JSON array of row objects instead of a table",
              },
            },
            async run({ args }) {
              await withPaths(dataDir, async (paths) => {
                const client = openReadOnly(dbFile(paths));
                let rows: Rows;
                try {
                  rows = runQuery(client, args.query);
                } finally {
                  client.close();
                }
                await emit(args.json === true ? rowsJson(rows) : rowsTable(rows));
              });
            },
          }),
        },
      }),
      completions: defineCommand({
        meta: { name: "completions", description: "Print a shell completion script to stdout" },
        args: {
          shell: { type: "positional", required: true, description: "bash, zsh, or fish" },
        },
        // Runs before any config loads, so it works during install.
        async run({ args }) {
          await emit(completionScript(parseShell(args.shell)));
        },
      }),
    },
  });
  // Adds the `complete` command that the generated scripts call on every tab press.
  await tab(root);
  return root;
}

/** Config, dirs, and telemetry for every command but `config show` and `completions`. */
async function start(
  dataDir: string | undefined,
  beforeTelemetry?: (paths: Paths) => void,
): Promise<{ readonly config: Config; readonly paths: Paths; readonly telemetry: Telemetry }> {
  const config = loadConfig();
  const env = pathEnv();
  const paths = resolvePaths(dataDir, env);
  createPaths(paths);
  beforeTelemetry?.(paths);
  const telemetry = await initTelemetry(config, paths);
  // Resolution ran before the logger existed, so the override line is logged here.
  if (dataDir !== undefined && env.dataDir !== undefined) {
    log.debug("--data-dir overrides TRIAGE_DATA_DIR", { flag: dataDir, env: env.dataDir });
  }
  log.debug("startup", { config: Object.fromEntries(configEntries(config)), paths });
  return { config, paths, telemetry };
}

/** Runs `fn` with dirs and telemetry set up, and closes telemetry after it. */
async function withPaths(
  dataDir: string | undefined,
  fn: (paths: Paths, config: Config) => Promise<void>,
): Promise<void> {
  const { config, paths, telemetry } = await start(dataDir);
  try {
    await fn(paths, config);
  } finally {
    await telemetry.close();
  }
}

async function serveCommand(dataDir: string | undefined): Promise<void> {
  const signals = shutdownSignals();
  let lock: Lock | undefined;
  let cut = false;
  try {
    // The lock comes before telemetry, so a second instance never writes into the running
    // one's log file.
    const { config, paths, telemetry } = await start(dataDir, (paths) => {
      lock = lockDataDir(paths);
    });
    try {
      cut = (await serve(config, paths, signals)) === "cut";
    } catch (err) {
      // Logged so it reaches the JSONL file; the log already prints it on stderr, so main stays quiet.
      log.error("serve failed", { error: err });
      process.exitCode = 1;
    } finally {
      await telemetry.close();
    }
  } finally {
    signals.dispose();
    lock?.release();
  }
  if (cut) {
    // Handlers still running would keep the process alive.
    process.exit();
  }
}

/** The two largest units: `59s`, `59m`, `3h12m`, `1d2h`. */
export function uptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor(seconds / 3600) % 24;
  const mins = Math.floor(seconds / 60) % 60;
  if (days > 0) {
    return `${days}d${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h${mins}m`;
  }
  return mins > 0 ? `${mins}m` : `${seconds}s`;
}

export function configTable(config: Config, paths: Paths): string {
  const rows = [
    ...configEntries(config),
    ["data_dir", paths.data],
    ["logs_dir", paths.logs],
    ["config_dir", paths.config],
  ];
  const width = Math.max(...rows.map(([key = ""]) => key.length));
  return rows.map(([key = "", value]) => `${key.padEnd(width)}  ${value}\n`).join("");
}

/** An aligned table with `NULL` for SQL null, names and cells escaped like the columns log format. */
export function rowsTable(rows: Rows): string {
  if (rows.columns.length === 0) {
    return "";
  }
  const header = rows.columns.map(oneLine);
  const body = rows.rows.map((row) =>
    row.map((value) => {
      if (value === null) {
        return "NULL";
      }
      return typeof value === "string" ? oneLine(value) : cellText(value);
    }),
  );
  const width = (text: string) => [...text].length;
  const widths = header.map((name, i) =>
    Math.max(width(name), ...body.map((line) => width(line[i] ?? ""))),
  );
  const rule = widths.map((w) => "-".repeat(w));
  return [header, rule, ...body]
    .map((line) => {
      const padded = line.map((text, i) => text + " ".repeat((widths[i] ?? 0) - width(text)));
      return `${padded.join("  ").trimEnd()}\n`;
    })
    .join("");
}

/** The first line where `live` differs from `want`, or `undefined` when they match. */
export function schemaDrift(want: string, live: string): string | undefined {
  const wantLines = lines(want);
  const liveLines = lines(live);
  const count = Math.max(wantLines.length, liveLines.length);
  for (let i = 0; i < count; i++) {
    const a = wantLines[i];
    const b = liveLines[i];
    if (a !== b) {
      return `schema drift at line ${i + 1}\n  schema.sql  ${shown(a)}\n  database    ${shown(b)}\n`;
    }
  }
  return undefined;
}

// Rust's `str::lines`: a final newline ends the last line instead of starting an empty one.
function lines(text: string): string[] {
  const parts = text.split("\n").map((line) => line.replace(/\r$/, ""));
  if (parts.at(-1) === "") {
    parts.pop();
  }
  return parts;
}

function shown(line: string | undefined): string {
  if (line === undefined) {
    return "(end)";
  }
  return line === "" ? "(blank line)" : line;
}

function parseShell(shell: string): Shell {
  const known = SHELLS.find((candidate) => candidate === shell);
  if (known === undefined) {
    throw new CliError(`unknown shell ${shell}; use ${SHELLS.join(", ")}`, 2);
  }
  return known;
}

// tab prints its script through `console.log`; capture it so the output goes through `emit`.
function completionScript(shell: Shell): string {
  const lines: string[] = [];
  const log = console.log;
  console.log = (...parts: unknown[]) => {
    lines.push(parts.join(" "));
  };
  try {
    script(shell, "triage", "triage");
  } finally {
    console.log = log;
  }
  return `${lines.join("\n")}\n`;
}

/** Writes command output. A reader that closed the pipe early, as `head` does, is not an error. */
async function emit(text: string): Promise<void> {
  await writeOut(async (chunk) => {
    await Bun.write(Bun.stdout, chunk);
  }, text);
}

export async function writeOut(
  write: (text: string) => Promise<void>,
  text: string,
): Promise<void> {
  try {
    await write(text);
  } catch (err) {
    if (err instanceof Error && "code" in err && err.code === "EPIPE") {
      return;
    }
    const reason = err instanceof Error ? err.message : String(err);
    throw new CliError(`cannot write to stdout: ${reason}`, 1, { cause: err });
  }
}
