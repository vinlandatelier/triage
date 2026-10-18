import type { Database } from "bun:sqlite";
import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openStore, type Store } from "./open";
import { dumpSchema, SCHEMA } from "./schema";

const SCHEMA_FILE = join(import.meta.dir, "../../schema.sql");
const temps: string[] = [];
const stores: Store[] = [];

afterEach(() => {
  for (const store of stores.splice(0)) {
    store.close();
  }
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function migrated(): Database {
  const dir = mkdtempSync(join(tmpdir(), "triage-schema-"));
  temps.push(dir);
  const { store } = openStore(join(dir, "triage.db"));
  stores.push(store);
  return store.db.$client;
}

/**
 * Every table is `STRICT`, keyed by `id TEXT PRIMARY KEY`, carries `created_at` and
 * `updated_at`, and uses snake_case names.
 */
function violations(client: Database): string[] {
  const tables = client
    .query<{ name: string; strict: number }, []>(
      `SELECT name, strict FROM pragma_table_list
       WHERE schema = 'main' AND type = 'table'
         AND name NOT GLOB 'sqlite_*' AND name <> '_migrations'
       ORDER BY name`,
    )
    .all();
  return tables.flatMap(({ name, strict }) => tableViolations(client, name, strict !== 0));
}

function tableViolations(client: Database, table: string, strict: boolean): string[] {
  const found: string[] = [];
  if (!strict) {
    found.push(`${table}: not STRICT`);
  }
  if (!isSnakeCase(table)) {
    found.push(`${table}: table name is not snake_case`);
  }
  const columns = client
    .query<{ name: string; type: string; pk: number }, [string]>(
      "SELECT name, type, pk FROM pragma_table_xinfo(?) ORDER BY cid",
    )
    .all(table);
  const pk = columns.filter((column) => column.pk > 0);
  if (!(pk.length === 1 && pk[0]?.name === "id" && pk[0].type === "TEXT")) {
    found.push(`${table}: primary key is not \`id TEXT\``);
  }
  for (const required of ["created_at", "updated_at"]) {
    if (!columns.some((column) => column.name === required)) {
      found.push(`${table}: missing ${required}`);
    }
  }
  for (const { name } of columns) {
    if (!isSnakeCase(name)) {
      found.push(`${table}.${name}: column name is not snake_case`);
    }
  }
  return found;
}

function isSnakeCase(name: string): boolean {
  return /^[a-z][a-z0-9_]*$/.test(name);
}

test("schema conventions", () => {
  expect(violations(migrated())).toEqual([]);
});

test("violations flags each broken rule", () => {
  const client = migrated();
  client.run(`
    CREATE TABLE good (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, updated_at TEXT NOT NULL) STRICT;
    CREATE TABLE loose (id TEXT PRIMARY KEY, created_at TEXT, updated_at TEXT);
    CREATE TABLE "BadName" (id INTEGER PRIMARY KEY, "createdAt" TEXT) STRICT;
    CREATE TABLE any_id (id ANY PRIMARY KEY, created_at TEXT, updated_at TEXT) STRICT;
    CREATE TABLE composite (
      id TEXT, n INTEGER, created_at TEXT, updated_at TEXT,
      "genCol" TEXT GENERATED ALWAYS AS (id) VIRTUAL,
      PRIMARY KEY (id, n)
    ) STRICT;
    CREATE TABLE sqlitex (id TEXT PRIMARY KEY, created_at TEXT, updated_at TEXT);
  `);
  expect(violations(client)).toEqual([
    "BadName: table name is not snake_case",
    "BadName: primary key is not `id TEXT`",
    "BadName: missing created_at",
    "BadName: missing updated_at",
    "BadName.createdAt: column name is not snake_case",
    "any_id: primary key is not `id TEXT`",
    "composite: primary key is not `id TEXT`",
    "composite.genCol: column name is not snake_case",
    "loose: not STRICT",
    "sqlitex: not STRICT",
  ]);
});

test("the embedded schema is schema.sql", () => {
  expect(SCHEMA).toBe(readFileSync(SCHEMA_FILE, "utf8"));
});

test("schema.sql is current", () => {
  // On failure, run `just db-schema`.
  expect(dumpSchema(migrated())).toBe(SCHEMA);
});
