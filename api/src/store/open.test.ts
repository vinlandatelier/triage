import { Database } from "bun:sqlite";
import { afterEach, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { sql } from "drizzle-orm";
import { StoreError } from "./error";
import { openReadOnly, openStore } from "./open";

const temps: string[] = [];

afterEach(() => {
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "triage-store-"));
  temps.push(dir);
  return dir;
}

function migrations(dir: string, files: Record<string, string>): string {
  mkdirSync(dir, { recursive: true });
  for (const [name, text] of Object.entries(files)) {
    writeFileSync(join(dir, name), text);
  }
  return dir;
}

function failure(fn: () => unknown): StoreError {
  try {
    fn();
  } catch (err) {
    if (err instanceof StoreError) {
      return err;
    }
    throw err;
  }
  throw new Error("expected a StoreError");
}

test("creates the file in WAL mode with the pragmas", () => {
  const dir = tempDir();
  const file = join(dir, "triage.db");
  const { store } = openStore(file, migrations(join(dir, "m"), {}));
  expect(existsSync(file)).toBe(true);
  const pragma = (name: string) => store.db.$client.query(`PRAGMA ${name}`).values()[0]?.[0];
  expect(pragma("journal_mode")).toBe("wal");
  expect([pragma("synchronous"), pragma("foreign_keys"), pragma("temp_store")]).toEqual([1, 1, 2]);
  expect(pragma("busy_timeout")).toBe(5000);
  store.close();
});

test("counts only newly applied migrations", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const m = migrations(join(dir, "m"), {
    "0001_first.sql": "CREATE TABLE first (x INTEGER) STRICT;",
    "0002_second.sql": "CREATE TABLE second (x INTEGER) STRICT;",
  });
  const first = openStore(file, m);
  first.store.close();
  const second = openStore(file, m);
  second.store.close();
  expect([first.applied, second.applied]).toEqual([2, 0]);
});

test("records each migration with its sha-256 and a fixed-width time", () => {
  const dir = tempDir();
  const text = "CREATE TABLE first (x INTEGER) STRICT;";
  const { store } = openStore(
    join(dir, "t.db"),
    migrations(join(dir, "m"), { "0001_first.sql": text }),
  );
  const rows = store.db.$client
    .query<{ name: string; hash: string; appliedAt: string }, []>(
      "SELECT name, hash, applied_at AS appliedAt FROM _migrations",
    )
    .all();
  store.close();
  expect(rows).toEqual([
    {
      name: "0001_first.sql",
      hash: new Bun.CryptoHasher("sha256").update(text).digest("hex"),
      appliedAt: expect.stringMatching(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/),
    },
  ]);
});

test("a failed migration keeps earlier files and names itself", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const m = migrations(join(dir, "m"), {
    "0001_first.sql": "CREATE TABLE first (x INTEGER) STRICT;",
    "0002_broken_one.sql": "CREATE TABLE second (x INTEGER) STRICT; SELECT nope FROM nowhere;",
    "0003_third.sql": "CREATE TABLE third (x INTEGER) STRICT;",
  });
  expect(failure(() => openStore(file, m)).message).toContain("0002_broken_one.sql");
  const client = new Database(file);
  const tables = client
    .query("SELECT name FROM sqlite_schema WHERE name IN ('first', 'second', 'third')")
    .values();
  client.close();
  expect(tables).toEqual([["first"]]);
});

test("an edited migration names itself and checksum", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const m = migrations(join(dir, "m"), { "0001_first.sql": "CREATE TABLE first (x INTEGER);" });
  openStore(file, m).store.close();
  migrations(m, { "0001_first.sql": "CREATE TABLE first (y INTEGER);" });
  const message = failure(() => openStore(file, m)).message;
  expect(message).toContain("0001_first.sql");
  expect(message).toContain("checksum");
});

test("a newer database names the unknown migration", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const first = "CREATE TABLE first (x INTEGER) STRICT;";
  const newer = migrations(join(dir, "new"), {
    "0001_first.sql": first,
    "0002_second.sql": "CREATE TABLE second (x INTEGER) STRICT;",
  });
  openStore(file, newer).store.close();
  const older = migrations(join(dir, "old"), { "0001_first.sql": first });
  expect(failure(() => openStore(file, older)).message).toBe(
    "the database has migration 0002_second.sql, which this triage does not know; upgrade triage",
  );
});

test("a database from the Rust build is refused", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const client = new Database(file);
  client.run("CREATE TABLE _sqlx_migrations (version INTEGER PRIMARY KEY)");
  client.close();
  expect(failure(() => openStore(file, migrations(join(dir, "m"), {}))).message).toBe(
    `database created by an older triage build; delete ${file} to start fresh`,
  );
});

test("a file that is not a database names the path", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  writeFileSync(file, "this is not sqlite, just enough bytes to fill a header....");
  const err = failure(() => openStore(file, migrations(join(dir, "m"), {})));
  expect(err.exitCode).toBe(1);
  expect(err.message).toContain(`cannot open database ${file}`);
  expect(failure(() => openReadOnly(file)).message).toContain(`cannot open database ${file}`);
});

test("close removes the wal files", () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const { store } = openStore(
    file,
    migrations(join(dir, "m"), { "0001_t.sql": "CREATE TABLE t (x INTEGER) STRICT;" }),
  );
  store.write((tx) => tx.run(sql`INSERT INTO t VALUES (1)`));
  expect(existsSync(`${file}-wal`)).toBe(true);
  store.close();
  expect([existsSync(`${file}-wal`), existsSync(`${file}-shm`)]).toEqual([false, false]);
});

test("write commits, and rolls back when fn throws", () => {
  const dir = tempDir();
  const { store } = openStore(
    join(dir, "t.db"),
    migrations(join(dir, "m"), { "0001_t.sql": "CREATE TABLE t (x INTEGER) STRICT;" }),
  );
  store.write((tx) => tx.run(sql`INSERT INTO t VALUES (1)`));
  expect(() =>
    store.write((tx) => {
      tx.run(sql`INSERT INTO t VALUES (2)`);
      throw new Error("abort");
    }),
  ).toThrow("abort");
  expect(store.db.$client.query("SELECT x FROM t").values()).toEqual([[1]]);
  store.close();
});

test("the read-only connection rejects writes and never creates a missing file", () => {
  const dir = tempDir();
  const file = join(dir, "triage.db");
  expect(failure(() => openReadOnly(file)).message).toBe(
    `no database at ${file}; run \`triage db migrate\` first`,
  );
  expect(existsSync(file)).toBe(false);
  openStore(file, migrations(join(dir, "m"), {})).store.close();
  const client = openReadOnly(file);
  expect(() => client.run("DELETE FROM _migrations")).toThrow("readonly database");
  client.close();
});

test("write rejects an async fn and rolls it back", () => {
  const dir = tempDir();
  const { store } = openStore(
    join(dir, "t.db"),
    migrations(join(dir, "m"), { "0001_t.sql": "CREATE TABLE t (x INTEGER) STRICT;" }),
  );
  expect(() =>
    store.write((tx) => {
      tx.run(sql`INSERT INTO t VALUES (1)`);
      return Promise.resolve();
    }),
  ).toThrow("store.write needs a synchronous fn");
  expect(store.db.$client.query("SELECT x FROM t").values()).toEqual([]);
  store.close();
});

test("two processes migrating at once apply each file once", async () => {
  const dir = tempDir();
  const file = join(dir, "t.db");
  const m = migrations(join(dir, "m"), {
    // Slow enough that the second process starts while the first holds the write lock.
    "0001_slow.sql": `CREATE TABLE slow (n INTEGER) STRICT;
      INSERT INTO slow WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000)
      SELECT count(*) FROM c;`,
  });
  const script = join(dir, "migrate.ts");
  writeFileSync(
    script,
    `import { openStore } from ${JSON.stringify(join(import.meta.dir, "open.ts"))};
const { store, applied } = openStore(Bun.argv[2], Bun.argv[3]);
store.close();
await Bun.write(Bun.stdout, String(applied));`,
  );
  const run = async () => {
    const proc = Bun.spawn([process.execPath, script, file, m], { stdout: "pipe" });
    const [out, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited]);
    return `${code}:${out}`;
  };
  expect((await Promise.all([run(), run()])).sort()).toEqual(["0:0", "0:1"]);
  const client = new Database(file);
  expect(client.query("SELECT count(*) FROM _migrations").values()).toEqual([[1]]);
  client.close();
});
