import type { Database } from "bun:sqlite";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { reason, StoreError } from "./error";
import { now, toDbTime } from "./time";

export const MIGRATIONS_DIR = join(import.meta.dir, "../../migrations");

type Migration = { readonly name: string; readonly hash: string; readonly sql: string };

/**
 * Applies every pending `*.sql` file in `dir` in name order, each in its own `BEGIN IMMEDIATE`
 * transaction, and returns how many it applied. Drizzle's `migrate()` wraps all files in one
 * transaction and checks neither hashes nor rows the folder lacks.
 */
export function migrate(client: Database, file: string, dir: string): number {
  if (hasTable(client, "_sqlx_migrations")) {
    throw new StoreError(
      `database created by an older triage build; delete ${file} to start fresh`,
    );
  }
  client.run(
    "CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY, hash TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT",
  );
  const migrations = readMigrations(dir);
  checkApplied(client, migrations);
  let count = 0;
  for (const migration of migrations) {
    if (apply(client, migration)) {
      count++;
    }
  }
  return count;
}

function readMigrations(dir: string): Migration[] {
  return readdirSync(dir)
    .filter((name) => name.endsWith(".sql"))
    .sort()
    .map((name) => {
      const bytes = readFileSync(join(dir, name));
      const hash = new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
      return { name, hash, sql: new TextDecoder().decode(bytes) };
    });
}

function checkApplied(client: Database, migrations: readonly Migration[]): void {
  const known = new Map(migrations.map((migration) => [migration.name, migration.hash]));
  const rows = client
    .query<{ name: string; hash: string }, []>("SELECT name, hash FROM _migrations ORDER BY name")
    .all();
  for (const row of rows) {
    const hash = known.get(row.name);
    if (hash === undefined) {
      throw new StoreError(
        `the database has migration ${row.name}, which this triage does not know; upgrade triage`,
      );
    }
    if (hash !== row.hash) {
      throw new StoreError(
        `migration ${row.name} was edited after it was applied: its checksum no longer matches`,
      );
    }
  }
}

// The applied check runs inside the write lock, so a second process that waited on
// `busy_timeout` finds the file applied and skips it.
function apply(client: Database, migration: Migration): boolean {
  try {
    return client
      .transaction(() => {
        const applied = client
          .query<{ one: number }, [string]>("SELECT 1 AS one FROM _migrations WHERE name = ?")
          .get(migration.name);
        if (applied !== null) {
          return false;
        }
        client.run(migration.sql);
        client.run("INSERT INTO _migrations (name, hash, applied_at) VALUES (?, ?, ?)", [
          migration.name,
          migration.hash,
          toDbTime(now()),
        ]);
        return true;
      })
      .immediate();
  } catch (err) {
    throw new StoreError(`migration ${migration.name} failed: ${reason(err)}`, { cause: err });
  }
}

function hasTable(client: Database, name: string): boolean {
  return (
    client
      .query<{ one: number }, [string]>(
        "SELECT 1 AS one FROM sqlite_schema WHERE type = 'table' AND name = ?",
      )
      .get(name) !== null
  );
}
