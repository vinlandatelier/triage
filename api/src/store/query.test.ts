import { Database } from "bun:sqlite";
import { afterEach, expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { StoreError } from "./error";
import { type Rows, rowsJson, runQuery, scanStatements } from "./query";

const clients: Database[] = [];

afterEach(() => {
  for (const client of clients.splice(0)) {
    client.close();
  }
});

function memory(): Database {
  const client = new Database(":memory:", { safeIntegers: true });
  clients.push(client);
  return client;
}

function rejection(sql: string): string {
  try {
    runQuery(memory(), sql);
  } catch (err) {
    if (err instanceof StoreError) {
      return err.message;
    }
    throw err;
  }
  throw new Error(`${JSON.stringify(sql)} must be rejected`);
}

test("decodes each storage class", () => {
  const rows = runQuery(memory(), "SELECT 1 AS i, 1.5 AS r, 'a' AS t, x'00ff' AS b, NULL AS n");
  expect(rows).toEqual({
    columns: ["i", "r", "t", "b", "n"],
    rows: [[1n, 1.5, "a", "x'00ff'", null]],
  });
  expect(rowsJson(rows)).toBe(`[
  {
    "b": "x'00ff'",
    "i": 1,
    "n": null,
    "r": 1.5,
    "t": "a"
  }
]
`);
});

test("decodes the storage class of each cell, not each column", () => {
  const rows = runQuery(memory(), "VALUES (1), ('a'), (x'01'), (2.5), (NULL)");
  expect(rows.rows).toEqual([[1n], ["a"], ["x'01'"], [2.5], [null]]);
});

test("keeps invalid UTF-8, big integers, and non-finite reals", () => {
  const rows = runQuery(
    memory(),
    "SELECT CAST(x'61ff' AS TEXT), 9007199254740993, -9223372036854775808, 1e999, -1e999",
  );
  expect(rows.rows).toEqual([
    ["a\u{fffd}", 9007199254740993n, -9223372036854775808n, "inf", "-inf"],
  ]);
  expect(rowsJson(rows)).toContain('"9007199254740993": 9007199254740993');
});

test("prints reals as Rust did", () => {
  const rows = runQuery(
    memory(),
    "SELECT 1.0 AS a, -0.0 AS b, 1e16 AS c, 1e15 AS c15, 1e17 AS d, 0.0001 AS e, 1.5e-7 AS f, 123.456 AS g, 2.5e300 AS h",
  );
  expect(rowsJson(rows)).toBe(`[
  {
    "a": 1.0,
    "b": -0.0,
    "c": 1e+16,
    "c15": 1000000000000000.0,
    "d": 1e+17,
    "e": 0.0001,
    "f": 1.5e-7,
    "g": 123.456,
    "h": 2.5e+300
  }
]
`);
});

test("gives duplicate columns distinct names when wrapped", () => {
  expect(runQuery(memory(), "SELECT 1 AS a, 2 AS a").columns).toEqual(["a", "a:1"]);
});

test("rejects an empty column name instead of shifting the headers", () => {
  expect(rejection('SELECT 1 AS "", 2 AS b')).toBe(
    "query failed: a column has an empty name; alias it",
  );
});

test("keeps columns when no rows match", () => {
  const rows = runQuery(memory(), "SELECT 1 AS a, 2 AS b WHERE 0");
  expect(rows).toEqual({ columns: ["a", "b"], rows: [] });
  expect(rowsJson(rows)).toBe("[]\n");
});

test("rejects empty and multi-statement input", () => {
  for (const sql of ["", " ; -- nothing"]) {
    expect(rejection(sql)).toBe("the query is empty");
  }
  for (const sql of ["SELECT 1; SELECT 2", "SELECT 1 AS a, 2 AS b; SELECT 3"]) {
    expect(rejection(sql)).toBe("pass exactly one statement");
  }
});

test("allows only reading statements", () => {
  const dir = mkdtempSync(join(tmpdir(), "triage-query-"));
  const copy = join(dir, "copy.db");
  try {
    for (const sql of [
      `VACUUM INTO '${copy}'`,
      `ATTACH '${copy}' AS other`,
      "DELETE FROM t",
      "(SELECT 1)",
    ]) {
      expect(rejection(sql)).toBe(
        "only SELECT, WITH, EXPLAIN, VALUES, or PRAGMA statements run here",
      );
    }
    expect(existsSync(copy)).toBe(false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  for (const sql of [
    "select 1",
    "-- note\n/* x */ SELECT 1",
    "SELECT 1; -- trailing",
    "SELECT 1 -- no newline",
    "WITH a AS (SELECT 1) SELECT * FROM a",
    "EXPLAIN SELECT 1",
    "VALUES (1)",
    "PRAGMA user_version",
  ]) {
    expect(runQuery(memory(), sql).columns.length).toBeGreaterThan(0);
  }
});

test("reports SQLite's own error for a write behind WITH", () => {
  expect(rejection("WITH x AS (SELECT 1) DELETE FROM nowhere")).toContain("no such table: nowhere");
});

test("counts statements outside literals and comments", () => {
  const cases: [string, number][] = [
    ["SELECT 1;", 1],
    ["SELECT ';' AS \"a;b\", [c;d], `e;f`, 'it''s;'", 1],
    ["SELECT 1 -- ; SELECT 2\n;", 1],
    ["SELECT 1 /* ; */ ; /* trailing */", 1],
    ["-- only a comment", 0],
    ["SELECT 1;SELECT 2", 2],
    ["SELECT '--' ; SELECT 2", 2],
    ["SELECT 'abc; SELECT 2", 1],
    ["SELECT 1 /* x ; SELECT 2", 1],
    ["CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1; END", 2],
  ];
  for (const [sql, want] of cases) {
    expect([sql, scanStatements(sql).count]).toEqual([sql, want]);
  }
  expect(scanStatements("SELECT 1 ; -- x").end).toBe(9);
});

test("json rejects duplicate column names", () => {
  const rows: Rows = { columns: ["a", "a"], rows: [[1n, 2n]] };
  expect(() => rowsJson(rows)).toThrow(
    "column `a` appears twice; alias one so the JSON keys stay unique",
  );
});
