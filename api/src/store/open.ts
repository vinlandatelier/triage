import { constants, Database, type SQLQueryBindings, type Statement } from "bun:sqlite";
import { statSync } from "node:fs";
import { type BunSQLiteDatabase, drizzle } from "drizzle-orm/bun-sqlite";
import { reason, StoreError } from "./error";
import { MIGRATIONS_DIR, migrate } from "./migrate";

type Transaction = Parameters<Parameters<BunSQLiteDatabase["transaction"]>[0]>[0];

export type Store = {
  readonly db: BunSQLiteDatabase & { readonly $client: Database };
  /**
   * Runs `fn` in `BEGIN IMMEDIATE`, which takes the write lock up front: a deferred transaction
   * that reads first fails with `SQLITE_BUSY` when it upgrades, and `busy_timeout` cannot retry that.
   */
  write<T>(fn: (tx: Transaction) => T): T;
  /** Checkpoints the WAL and closes, which removes the `-wal` and `-shm` files. */
  close(): void;
};

const PRAGMAS = [
  "PRAGMA busy_timeout = 5000",
  "PRAGMA journal_mode = WAL",
  "PRAGMA synchronous = NORMAL",
  "PRAGMA foreign_keys = ON",
  "PRAGMA temp_store = MEMORY",
];

/**
 * Opens `file`, creating it if missing, and applies pending migrations from `migrations`.
 * bun:sqlite is synchronous, so one connection serves reads and writes.
 */
export function openStore(
  file: string,
  migrations: string = MIGRATIONS_DIR,
): { readonly store: Store; readonly applied: number } {
  const client = connect(file, () => {
    const client = new StoreDatabase(file, { create: true, strict: true });
    for (const pragma of PRAGMAS) {
      client.run(pragma);
    }
    // macOS's system SQLite, which Bun uses there, keeps the `-wal` file after close by default.
    client.fileControl(constants.SQLITE_FCNTL_PERSIST_WAL, 0);
    return client;
  });
  let applied: number;
  try {
    applied = migrate(client, file, migrations);
  } catch (err) {
    client.close();
    throw err;
  }
  const db = drizzle({ client });
  const store: Store = {
    db,
    write: (fn) =>
      db.transaction(
        (tx) => {
          const result = fn(tx);
          // The transaction is synchronous and would commit at the first `await`.
          if (result instanceof Promise) {
            throw new TypeError("store.write needs a synchronous fn");
          }
          return result;
        },
        { behavior: "immediate" },
      ),
    close() {
      client.run("PRAGMA wal_checkpoint(TRUNCATE)");
      client.close();
    },
  };
  return { store, applied };
}

/**
 * One read-only connection that never creates or migrates the file, so writes fail inside
 * SQLite itself. Integers come back as `bigint` so values past 2^53 stay exact.
 */
export function openReadOnly(file: string): Database {
  if (!isFile(file)) {
    throw new StoreError(`no database at ${file}; run \`triage db migrate\` first`);
  }
  return connect(file, () => {
    // A read-only connection cannot create the `-wal` and `-shm` files a WAL database needs, and
    // `Store.close` removes them. A connection that never writes opens them and stays open until
    // the read-only one holds them.
    const opener = new Database(file, { readwrite: true, create: false });
    try {
      opener.run("PRAGMA busy_timeout = 5000");
      // Opening is lazy, so a file that is not a database fails on its first read.
      opener.query("SELECT count(*) FROM sqlite_schema").get();
      const client = new Database(file, { readonly: true, safeIntegers: true });
      client.run("PRAGMA busy_timeout = 5000");
      client.run("PRAGMA query_only = ON");
      client.query("SELECT count(*) FROM sqlite_schema").get();
      return client;
    } finally {
      opener.close();
    }
  });
}

/**
 * Drizzle prepares a new statement for every query and never finalizes it, and SQLite defers a
 * close until every statement is finalized, so the `-wal` and `-shm` files would outlive
 * `close()`. This tracks each live statement weakly and finalizes the rest on close; finalizing
 * one twice is harmless.
 */
class StoreDatabase extends Database {
  readonly #statements = new Set<WeakRef<Statement>>();
  readonly #collected = new FinalizationRegistry<WeakRef<Statement>>((ref) => {
    this.#statements.delete(ref);
  });

  override prepare<ReturnType, ParamsType extends SQLQueryBindings | SQLQueryBindings[]>(
    sql: string,
    // `query()` and `transaction()` also pass prepare flags, which the rest forwards at runtime.
    ...rest: [params?: ParamsType]
    // biome-ignore lint/suspicious/noExplicitAny: an override must repeat bun-types' signature.
  ): Statement<ReturnType, ParamsType extends any[] ? ParamsType : [ParamsType]> {
    const statement = super.prepare<ReturnType, ParamsType>(sql, ...rest);
    const ref = new WeakRef<Statement>(statement);
    this.#statements.add(ref);
    this.#collected.register(statement, ref);
    return statement;
  }

  override close(throwOnError?: boolean): void {
    for (const ref of this.#statements) {
      ref.deref()?.finalize();
    }
    this.#statements.clear();
    super.close(throwOnError);
  }
}

function connect(file: string, open: () => Database): Database {
  try {
    return open();
  } catch (err) {
    throw new StoreError(`cannot open database ${file}: ${reason(err)}`, { cause: err });
  }
}

function isFile(path: string): boolean {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}
