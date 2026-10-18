import { Database, SQLiteError } from "bun:sqlite";
import { join } from "node:path";
import packageJson from "../../package.json" with { type: "json" };
import type { Config } from "../config";
import { BODY_LIMIT } from "../error";
import { dbFile, type Paths } from "../paths";
import { reason } from "../store/error";
import { openStore } from "../store/open";
import { logger } from "../telemetry";
import { logFile } from "../telemetry/daily-file";
import { createApp } from "./app";

const DRAIN_MS = 10_000;
// Bun answers a body over this with a bare 413 before the app sees it, and never limits a
// chunked body. Hono's `bodyLimit` is the real cap, so this only stops absurd uploads.
const BUN_BODY_LIMIT = 8 * BODY_LIMIT;

const log = logger("http");

/** A failure to start or run the server. Every one exits 1. */
export class ServeError extends Error {
  readonly exitCode = 1;

  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "ServeError";
  }
}

/** Proof that this process owns the data dir. The OS drops it when the process dies. */
export type Lock = { readonly release: () => void };

/**
 * Holds `BEGIN EXCLUSIVE` on `<data>/triage.lock` until released. Bun has no `flock`, and an
 * SQLite exclusive lock is an OS lock that never goes stale after a crash.
 */
export function lockDataDir(paths: Paths): Lock {
  const file = join(paths.data, "triage.lock");
  let client: Database | undefined;
  try {
    client = new Database(file, { create: true });
    client.run("BEGIN EXCLUSIVE");
  } catch (err) {
    client?.close();
    if (err instanceof SQLiteError && err.code === "SQLITE_BUSY") {
      throw new ServeError(`another triage instance is using ${paths.data}`, { cause: err });
    }
    throw new ServeError(`cannot lock ${file}: ${reason(err)}`, { cause: err });
  }
  const held = client;
  return { release: () => held.close() };
}

/** Delivers each SIGINT or SIGTERM, including ones that arrived before anyone waited. */
export type Signals = { readonly next: () => Promise<void> };

export function signalQueue(): Signals & { readonly send: () => void } {
  let pending = 0;
  const waiters: (() => void)[] = [];
  return {
    send() {
      const waiter = waiters.shift();
      if (waiter === undefined) {
        pending += 1;
      } else {
        waiter();
      }
    },
    next() {
      if (pending > 0) {
        pending -= 1;
        return Promise.resolve();
      }
      return new Promise((resolve) => {
        waiters.push(resolve);
      });
    },
  };
}

/** Registered before the store opens and the port binds, so a signal during startup still drains. */
export function shutdownSignals(): Signals & { readonly dispose: () => void } {
  const queue = signalQueue();
  const onSignal = () => queue.send();
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);
  return {
    next: queue.next,
    dispose() {
      process.off("SIGINT", onSignal);
      process.off("SIGTERM", onSignal);
    },
  };
}

export type Stopped = "drained" | "cut";

/**
 * Serves on `127.0.0.1:config.port` until a signal, then drains for up to 10 s. Returns `cut`
 * when the drain ran out or a second signal arrived, so the caller exits without waiting.
 */
export async function serve(config: Config, paths: Paths, signals: Signals): Promise<Stopped> {
  const startedAt = performance.now();
  const { store } = openStore(dbFile(paths));
  let server: Bun.Server<undefined>;
  try {
    server = listen(config.port, createApp({ startedAt, store }).fetch);
  } catch (err) {
    store.close();
    throw err;
  }
  log.info("listening", {
    version: packageJson.version,
    addr: `127.0.0.1:${server.port}`,
    data: paths.data,
    db: dbFile(paths),
    log: logFile(paths.logs, new Date()),
  });
  try {
    const stopped = await serveUntil(server, signals, DRAIN_MS);
    // A cut drain leaves handlers running that may still use the store.
    if (stopped === "drained") {
      store.close();
    }
    return stopped;
  } finally {
    log.info("stopped", { uptimeS: Math.floor((performance.now() - startedAt) / 1000) });
  }
}

export function listen(
  port: number,
  fetch: (request: Request) => Response | Promise<Response>,
): Bun.Server<undefined> {
  try {
    return Bun.serve({
      // Bun binds every interface by default.
      hostname: "127.0.0.1",
      port,
      maxRequestBodySize: BUN_BODY_LIMIT,
      fetch,
    });
  } catch (err) {
    if (err instanceof Error && "code" in err && err.code === "EADDRINUSE") {
      throw new ServeError(`port ${port} is already in use; set TRIAGE_PORT to a free port`, {
        cause: err,
      });
    }
    throw new ServeError(`cannot listen on 127.0.0.1:${port}: ${reason(err)}`, { cause: err });
  }
}

/** Stops accepting at the first signal, then waits up to `drainMs` or until the next signal. */
export async function serveUntil(
  server: Bun.Server<undefined>,
  signals: Signals,
  drainMs: number,
): Promise<Stopped> {
  await signals.next();
  log.info("shutting down", { drainS: drainMs / 1000 });
  let timer: Timer | undefined;
  const stopped = await Promise.race([
    server.stop().then(() => "drained" as const),
    new Promise<Stopped>((resolve) => {
      timer = setTimeout(() => {
        log.warn("open requests did not finish within the drain window");
        resolve("cut");
      }, drainMs);
    }),
    signals.next().then(() => {
      log.warn("second signal: exiting without waiting for open requests");
      return "cut" as const;
    }),
  ]);
  clearTimeout(timer);
  return stopped;
}
