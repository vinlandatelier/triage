import { readdirSync, rmSync } from "node:fs";
import { pino } from "pino";

const KEEP = 7;
const NAME = /^triage\.\d{4}-\d{2}-\d{2}\.jsonl$/;

type Destination = ReturnType<typeof pino.destination>;

export type DailyFile = {
  readonly write: (line: string) => void;
  readonly close: () => void;
};

/** The JSONL file for the UTC day of `now`. */
export function logFile(logs: string, now: Date): string {
  return `${logs}/triage.${now.toISOString().slice(0, 10)}.jsonl`;
}

/**
 * Opens today's file, or throws when the dir is unusable. Swaps to the next day's file on the first
 * write after UTC midnight and keeps the newest {@link KEEP}. The first error calls `onError`.
 */
export function openDailyFile(
  logs: string,
  onError: (err: unknown) => void,
  now: () => Date = () => new Date(),
): DailyFile {
  let path = logFile(logs, now());
  let warned = false;
  // After a failed write sonic-boom keeps every later line in an unbounded retry buffer, so a full
  // disk would grow memory forever. Drop lines until the next file opens instead.
  let broken = false;
  const fail = (err: unknown) => {
    broken = true;
    if (!warned) {
      warned = true;
      onError(err);
    }
  };
  let dest = open(path, fail);
  prune(logs, path);
  return {
    write(line) {
      try {
        const today = logFile(logs, now());
        if (today !== path) {
          // Opened first: if it throws, the next write retries.
          const next = open(today, fail);
          end(dest);
          dest = next;
          path = today;
          broken = false;
          prune(logs, path);
        }
        if (!broken) {
          dest.write(line);
        }
      } catch (err) {
        fail(err);
      }
    },
    close() {
      if (!broken) {
        try {
          dest.flushSync();
        } catch (err) {
          fail(err);
        }
      }
      end(dest);
    },
  };
}

function open(path: string, fail: (err: unknown) => void): Destination {
  const dest = pino.destination({ dest: path, sync: true, append: true, mkdir: false });
  dest.on("error", fail);
  return dest;
}

function end(dest: Destination): void {
  try {
    dest.end();
  } catch {
    // Its error already reached `fail`.
  }
}

// Best effort, by name, which is date order. The open file is never deleted, even if the clock
// went back. Another triage process may prune the same dir at the same moment.
function prune(logs: string, current: string): void {
  try {
    const old = readdirSync(logs)
      .filter((name) => NAME.test(name) && `${logs}/${name}` !== current)
      .sort()
      .slice(0, -(KEEP - 1));
    for (const name of old) {
      rmSync(`${logs}/${name}`, { force: true });
    }
  } catch {
    // A file left over is harmless; failing the open or the write is not.
  }
}
