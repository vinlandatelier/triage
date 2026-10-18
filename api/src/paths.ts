import {
  chmodSync,
  closeSync,
  fstatSync,
  mkdirSync,
  openSync,
  statSync,
  unlinkSync,
} from "node:fs";
import { isAbsolute } from "node:path";
import { ConfigError, type PathEnv } from "./config";

export type Paths = {
  readonly data: string;
  readonly logs: string;
  readonly config: string;
};

/** `--data-dir` > `TRIAGE_DATA_DIR` > platform dirs. Touches nothing on disk; see {@link createPaths}. */
export function resolvePaths(
  flag: string | undefined,
  env: PathEnv,
  platform: NodeJS.Platform = process.platform,
): Paths {
  const dir = flag ?? env.dataDir;
  return dir === undefined ? platformPaths(env, platform) : pathsUnder(absoluteDataDir(dir));
}

/** Creates each dir with mode 0700 and checks that the data and config dirs are writable. */
export function createPaths(paths: Paths): void {
  for (const dir of [paths.data, paths.config]) {
    createDir(dir);
    probeWritable(dir);
  }
  try {
    createDir(paths.logs);
  } catch {
    // Telemetry reports it.
  }
}

export function dbFile(paths: Paths): string {
  return join(paths.data, "triage.db");
}

function pathsUnder(dir: string): Paths {
  return { data: dir, logs: join(dir, "logs"), config: join(dir, "config") };
}

// An XDG var counts only when absolute, as the spec says; HOME must be absolute too, because a
// passwd fallback would silently pick a dir the user did not ask for.
function platformPaths(env: PathEnv, platform: NodeJS.Platform): Paths {
  const home = env.home;
  if (home === undefined || !isAbsolute(home)) {
    throw new ConfigError("cannot find a home directory; pass --data-dir <path>", 1);
  }
  if (platform === "darwin") {
    const data = join(home, "Library/Application Support/studio.triage");
    return { data, logs: join(home, "Library/Logs/triage"), config: data };
  }
  const xdg = (value: string | undefined, fallback: string) =>
    value !== undefined && isAbsolute(value) ? value : join(home, fallback);
  return {
    data: join(xdg(env.xdgDataHome, ".local/share"), "triage"),
    logs: join(xdg(env.xdgStateHome, ".local/state"), "triage/logs"),
    config: join(xdg(env.xdgConfigHome, ".config"), "triage"),
  };
}

// No shell expands `~` in `.env` or a quoted flag, and a relative dir would move with any chdir.
function absoluteDataDir(dir: string): string {
  if (dir.startsWith("~")) {
    throw new ConfigError(`data dir ${dir}: \`~\` is not expanded; use an absolute path`, 2);
  }
  return absolute(dir);
}

// Rust's `std::path::absolute`, so `config show` prints the same bytes: it drops `.` and empty
// segments but keeps `..` and a trailing slash, where `path.resolve` would rewrite both.
function absolute(dir: string): string {
  const full = isAbsolute(dir) ? dir : `${process.cwd()}/${dir}`;
  const segments = full.split("/").filter((segment) => segment !== "" && segment !== ".");
  const trailing = dir.endsWith("/") && segments.length > 0 ? "/" : "";
  // POSIX gives exactly two leading slashes an implementation-defined meaning, so Rust keeps them.
  const root = /^\/\/(?!\/)/.test(full) ? "//" : "/";
  return `${root}${segments.join("/")}${trailing}`;
}

function join(dir: string, child: string): string {
  return dir.endsWith("/") ? `${dir}${child}` : `${dir}/${child}`;
}

function createDir(dir: string): void {
  try {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  } catch (err) {
    throw new ConfigError(`cannot create directory ${dir}: ${message(err)}`, 1, { cause: err });
  }
}

// An existing read-only dir passes `mkdir`, so probe with a real write. The pid keeps concurrent
// starts apart; the `wx` flag refuses a planted symlink. Also tightens `dir` to 0700 when the
// current user owns it, read from the probe file's owner.
function probeWritable(dir: string): void {
  const probe = `${join(dir, ".triage-write-check-")}${process.pid}`;
  let uid: number;
  try {
    const fd = openSync(probe, "wx", 0o600);
    try {
      uid = fstatSync(fd).uid;
    } finally {
      closeSync(fd);
      unlinkSync(probe);
    }
  } catch (err) {
    throw new ConfigError(`directory ${dir} is not writable: ${message(err)}`, 1, { cause: err });
  }
  restrict(dir, uid);
}

// `mkdir` leaves an existing dir's mode alone, so one made by hand may still be 0755.
function restrict(dir: string, uid: number): void {
  try {
    const stat = statSync(dir);
    if (stat.uid === uid && (stat.mode & 0o077) !== 0) {
      chmodSync(dir, 0o700);
    }
  } catch {
    // Best effort, as in Rust: writability already passed.
  }
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
