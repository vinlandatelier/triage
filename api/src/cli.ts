import { script } from "@bomb.sh/tab";
import tab from "@bomb.sh/tab/citty";
import { type ArgsDef, defineCommand, parseArgs, renderUsage, runCommand, runMain } from "citty";
import packageJson from "../package.json" with { type: "json" };
import { type Config, configEntries, loadConfig, pathEnv } from "./config";
import { type Paths, resolvePaths } from "./paths";

/** A command failure with its exit code. */
export class CliError extends Error {
  readonly exitCode: 1 | 2;

  constructor(message: string, exitCode: 1 | 2, options?: ErrorOptions) {
    super(message, options);
    this.name = "CliError";
    this.exitCode = exitCode;
  }
}

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

export async function run(rawArgs: string[]): Promise<void> {
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

async function rootCommand(dataDir: string | undefined) {
  const root = defineCommand({
    meta: {
      name: "triage",
      version: packageJson.version,
      description: "Local life-admin agent",
    },
    args: globalArgs,
    subCommands: {
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
