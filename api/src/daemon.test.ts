import { afterEach, expect, setDefaultTimeout, test } from "bun:test";
import { existsSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import packageJson from "../package.json" with { type: "json" };

const MAIN = join(import.meta.dir, "main.ts");
const WAIT_MS = 10_000;

setDefaultTimeout(20_000);
const temps: string[] = [];
const daemons: Bun.Subprocess[] = [];

afterEach(() => {
  for (const daemon of daemons.splice(0)) {
    daemon.kill("SIGKILL");
  }
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function vars(...pairs: [string, string][]): Record<string, string> {
  return Object.fromEntries(pairs);
}

function tempDir(): string {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "triage-daemon-")));
  temps.push(dir);
  return dir;
}

// From a temp cwd with only HOME set, so no `.env` or caller TRIAGE_* or OTEL_* var leaks in.
function spawn(dir: string, args: readonly string[], env: Record<string, string> = {}) {
  return Bun.spawn([process.execPath, MAIN, ...args, "--data-dir", dir], {
    cwd: tempDir(),
    env: { ...vars(["HOME", tmpdir()]), ...env },
    stdout: "pipe",
    stderr: "pipe",
  });
}

async function run(dir: string, args: readonly string[], env: Record<string, string> = {}) {
  const proc = spawn(dir, args, env);
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { exitCode, stdout, stderr };
}

type Daemon = {
  readonly proc: Bun.Subprocess<"ignore", "pipe", "pipe">;
  readonly port: number;
  /** Waits for a stderr line that passes `match`. */
  readonly line: (match: (line: string) => boolean) => Promise<string>;
};

/** Starts `triage serve` on a free port and waits for its `listening` line. */
async function serve(dir: string): Promise<Daemon> {
  // TRIAGE_PORT=0 exits 2, so take a free port and hand it over.
  const free = await freePort();
  free.server.close();
  const proc = Bun.spawn([process.execPath, MAIN, "serve", "--data-dir", dir], {
    cwd: tempDir(),
    env: vars(["HOME", tmpdir()], ["TRIAGE_PORT", String(free.port)]),
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  daemons.push(proc);
  const lines: string[] = [];
  let wake: () => void = () => undefined;
  void (async () => {
    let rest = "";
    for await (const chunk of proc.stderr.pipeThrough(new TextDecoderStream())) {
      rest += chunk;
      const parts = rest.split("\n");
      rest = parts.pop() ?? "";
      lines.push(...parts);
      wake();
    }
  })();
  const line = async (match: (line: string) => boolean) => {
    const deadline = performance.now() + WAIT_MS;
    for (;;) {
      const found = lines.find(match);
      if (found !== undefined) {
        return found;
      }
      if (performance.now() > deadline) {
        throw new Error(`no matching stderr line in:\n${lines.join("\n")}`);
      }
      await Promise.race([
        new Promise<void>((resolve) => {
          wake = resolve;
        }),
        Bun.sleep(50),
      ]);
    }
  };
  await line((text) => text.includes(" listening "));
  return { proc, port: free.port, line };
}

function jsonl(dir: string): string {
  const logs = join(dir, "logs");
  return readdirSync(logs)
    .map((name) => readFileSync(join(logs, name), "utf8"))
    .join("");
}

async function freePort(): Promise<{ readonly server: Server; readonly port: number }> {
  const server = createServer();
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  return { server, port: typeof addr === "object" && addr !== null ? addr.port : 0 };
}

test("a second instance on the same data dir exits 1 and leaves the running log alone", async () => {
  const dir = tempDir();
  await serve(dir);
  const second = await run(dir, ["serve"]);
  expect(second.exitCode).toBe(1);
  expect(second.stderr).toBe(`error: another triage instance is using ${dir}\n`);
  expect(jsonl(dir)).not.toContain("another triage instance");
});

test("SIGTERM exits 0, logs stopped, and removes -wal and -shm", async () => {
  const dir = tempDir();
  const daemon = await serve(dir);
  expect((await fetch(`http://127.0.0.1:${daemon.port}/health`)).status).toBe(200);
  daemon.proc.kill("SIGTERM");
  expect(await daemon.proc.exited).toBe(0);
  await daemon.line((text) => text.includes(" stopped "));
  expect(existsSync(join(dir, "triage.db"))).toBe(true);
  expect(existsSync(join(dir, "triage.db-wal"))).toBe(false);
  expect(existsSync(join(dir, "triage.db-shm"))).toBe(false);
});

test("a second SIGTERM during shutdown still exits 0", async () => {
  const daemon = await serve(tempDir());
  daemon.proc.kill("SIGTERM");
  daemon.proc.kill("SIGTERM");
  expect(await daemon.proc.exited).toBe(0);
});

test("a port in use exits 1 and logs serve failed once", async () => {
  const dir = tempDir();
  const { server, port } = await freePort();
  try {
    const out = await run(
      dir,
      ["serve"],
      vars(["TRIAGE_PORT", String(port)], ["TRIAGE_LOG_LEVEL", "info"]),
    );
    expect(out.exitCode).toBe(1);
    expect(out.stderr.split("already in use").length - 1).toBe(1);
    expect(out.stderr).toContain("TRIAGE_PORT");
    const failed = jsonl(dir)
      .split("\n")
      .filter((line) => line.includes("serve failed"));
    expect(failed).toHaveLength(1);
    expect(failed[0]).toContain(`port ${port} is already in use`);
  } finally {
    server.close();
  }
});

test("one request id links the response, the stderr span close, and the JSONL line", async () => {
  const dir = tempDir();
  const daemon = await serve(dir);
  const response = await fetch(`http://127.0.0.1:${daemon.port}/health?x=1`);
  expect(response.status).toBe(200);
  const id = response.headers.get("x-request-id") ?? "missing";

  const close = await daemon.line((text) => text.endsWith(`  req=${id}`));
  expect(close).toMatch(/ INFO {2}http +http {2}/);
  for (const part of ["method=GET", "path=/health ", "status=200"]) {
    expect(close).toContain(part);
  }
  expect(close).not.toContain("?");
  expect(close.slice(0, -`  req=${id}`.length).split("  ").at(-1)).toMatch(/s$/);

  daemon.proc.kill("SIGTERM");
  await daemon.proc.exited;
  const line = jsonl(dir)
    .split("\n")
    .filter((text) => text !== "")
    .map((text) => JSON.parse(text) as Record<string, unknown>)
    .find((entry) => entry.request_id === id);
  expect(line).toMatchObject({ span: "http", status: 200, message: "response" });
});

test("status reports the version, port, and uptime of a running daemon", async () => {
  const dir = tempDir();
  const daemon = await serve(dir);
  const out = await run(tempDir(), ["status"], vars(["TRIAGE_PORT", String(daemon.port)]));
  expect(out.stdout).toMatch(
    new RegExp(`^running v${packageJson.version} on 127\\.0\\.0\\.1:${daemon.port}, up \\d+s\\n$`),
  );
  expect(out.exitCode).toBe(0);
});

test("status with nothing on the port prints not running and exits 3", async () => {
  const { server, port } = await freePort();
  server.close();
  const out = await run(tempDir(), ["status"], vars(["TRIAGE_PORT", String(port)]));
  expect(out.stdout).toBe("not running\n");
  expect(out.exitCode).toBe(3);
});

test("status against another service on the port is not running", async () => {
  const server = createServer((socket) => {
    socket.once("data", () => socket.end("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n"));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  const port = typeof addr === "object" && addr !== null ? addr.port : 0;
  try {
    const out = await run(tempDir(), ["status"], vars(["TRIAGE_PORT", String(port)]));
    expect(out.stdout).toBe("not running\n");
    expect(out.exitCode).toBe(3);
  } finally {
    server.close();
  }
});
