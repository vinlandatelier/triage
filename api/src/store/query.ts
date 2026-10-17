import type { Database } from "bun:sqlite";
import { reason, StoreError } from "./error";

/** A decoded cell. A `number` is always a finite real; blobs and non-finite reals are strings. */
type Cell = null | bigint | number | string;

/** Column names in select order, then one cell per column per row. */
export type Rows = { readonly columns: readonly string[]; readonly rows: readonly Cell[][] };

const READ_KEYWORDS = ["SELECT", "WITH", "EXPLAIN", "VALUES", "PRAGMA"];
const WRAPPED_KEYWORDS = ["SELECT", "WITH", "VALUES"];

/**
 * Runs exactly one reading statement on `client` and decodes each cell by its storage class.
 * User SQL never reaches `run` or `exec`, which would run every statement in the string.
 */
export function runQuery(client: Database, sql: string): Rows {
  const scan = scanStatements(sql);
  if (scan.count === 0) {
    throw new StoreError("the query is empty");
  }
  if (scan.count > 1) {
    throw new StoreError("pass exactly one statement");
  }
  // The read-only connection stops writes to the database, but not `VACUUM INTO` or `ATTACH`,
  // which create other files.
  const keyword = leadingKeyword(sql).toUpperCase();
  if (!READ_KEYWORDS.includes(keyword)) {
    throw new StoreError("only SELECT, WITH, EXPLAIN, VALUES, or PRAGMA statements run here");
  }
  const body = sql.slice(0, scan.end);
  try {
    return (WRAPPED_KEYWORDS.includes(keyword) && wrapped(client, body)) || plain(client, body);
  } catch (err) {
    throw new StoreError(`query failed: ${reason(err)}`, { cause: err });
  }
}

/**
 * Wrapping gives honest, distinct column names (`a`, `a:1`), where `columnNames` would dedupe.
 * The second query also reads each text cell as a blob, because bun:sqlite turns text with
 * invalid UTF-8 into an empty string. `MATERIALIZED` evaluates the query once, so a
 * non-deterministic cell and its blob agree. Returns `undefined` when the wrapped form does not
 * prepare, as with `WITH … DELETE`, so the plain run reports SQLite's own error.
 */
function wrapped(client: Database, body: string): Rows | undefined {
  let columns: string[];
  let count: number;
  try {
    const names = client.prepare(`SELECT * FROM (${body}\n)`);
    columns = names.columnNames;
    count = names.declaredTypes.length;
    names.finalize();
  } catch {
    return undefined;
  }
  // bun:sqlite drops an empty name from `columnNames`, so the headers would shift.
  if (columns.length !== count) {
    throw new StoreError("a column has an empty name; alias it");
  }
  const aliases = columns.map((_, i) => `c${i}`);
  const select = aliases
    .map((c) => `${c}, CASE WHEN typeof(${c}) = 'text' THEN CAST(${c} AS BLOB) END`)
    .join(", ");
  const statement = client.prepare<unknown[], []>(
    `WITH q(${aliases.join(", ")}) AS MATERIALIZED (${body}\n) SELECT ${select} FROM q`,
  );
  try {
    const rows = statement.values().map((raw) =>
      columns.map((_, i) => {
        const value = raw[2 * i];
        const bytes = raw[2 * i + 1];
        return typeof value === "string" && bytes instanceof Uint8Array
          ? new TextDecoder().decode(bytes)
          : cell(value);
      }),
    );
    return { columns, rows };
  } finally {
    statement.finalize();
  }
}

function plain(client: Database, body: string): Rows {
  const statement = client.prepare<unknown[], []>(body);
  try {
    const rows = statement.values().map((raw) => raw.map(cell));
    return { columns: statement.columnNames, rows };
  } finally {
    statement.finalize();
  }
}

function cell(value: unknown): Cell {
  if (value === null || typeof value === "bigint" || typeof value === "string") {
    return value;
  }
  if (value instanceof Uint8Array) {
    return `x'${Buffer.from(value).toString("hex")}'`;
  }
  if (typeof value === "number") {
    // JSON has no infinity, and `null` would read as SQL `NULL`.
    return Number.isFinite(value) ? value : realText(value);
  }
  return String(value);
}

/** Rows as JSON row objects with sorted keys, printed the way the Rust binary printed them. */
export function rowsJson(rows: Rows): string {
  const seen = new Set<string>();
  for (const name of rows.columns) {
    if (seen.has(name)) {
      throw new StoreError(
        `column \`${name}\` appears twice; alias one so the JSON keys stay unique`,
      );
    }
    seen.add(name);
  }
  if (rows.rows.length === 0) {
    return "[]\n";
  }
  // Sorted by UTF-8 bytes, as Rust's `BTreeMap<String, _>` sorts.
  const order = rows.columns
    .map((name, i) => ({ name, i, bytes: Buffer.from(name) }))
    .sort((a, b) => Buffer.compare(a.bytes, b.bytes));
  const objects = rows.rows.map((row) => {
    const fields = order.map(({ name, i }) => `    ${JSON.stringify(name)}: ${jsonCell(row[i])}`);
    return `  {\n${fields.join(",\n")}\n  }`;
  });
  return `[\n${objects.join(",\n")}\n]\n`;
}

function jsonCell(value: Cell | undefined): string {
  if (value === null || value === undefined) {
    return "null";
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  return cellText(value);
}

/** A non-string cell as the Rust binary printed it. */
export function cellText(value: bigint | number): string {
  return typeof value === "bigint" ? value.toString() : realText(value);
}

/**
 * Formats a real the way the Rust binary's serde_json did: shortest round-trip digits, `.0` on whole numbers,
 * and an exponent such as `1e+17` or `1e-7` outside the plain range.
 */
function realText(value: number): string {
  if (Number.isNaN(value)) {
    return "NaN";
  }
  if (!Number.isFinite(value)) {
    return value > 0 ? "inf" : "-inf";
  }
  const sign = value < 0 || Object.is(value, -0) ? "-" : "";
  if (value === 0) {
    return `${sign}0.0`;
  }
  const [mantissa = "", exponent = "0"] = Math.abs(value).toExponential().split("e");
  const digits = mantissa.replace(".", "");
  // The decimal point sits after `point` digits: 1.5e2 has digits 15 and point 3.
  const point = Number(exponent) + 1;
  if (point >= digits.length && point <= 16) {
    return `${sign}${digits}${"0".repeat(point - digits.length)}.0`;
  }
  if (point > 0 && point <= 16) {
    return `${sign}${digits.slice(0, point)}.${digits.slice(point)}`;
  }
  if (point > -5 && point <= 0) {
    return `${sign}0.${"0".repeat(-point)}${digits}`;
  }
  const fraction = digits.length > 1 ? `.${digits.slice(1)}` : "";
  const power = point - 1;
  return `${sign}${digits[0]}${fraction}e${power > 0 ? "+" : ""}${power}`;
}

const LETTER = /[A-Za-z]/;

/** The first word of `sql` after whitespace and comments. */
function leadingKeyword(sql: string): string {
  let rest = sql;
  for (;;) {
    rest = rest.trimStart();
    if (rest.startsWith("--")) {
      const end = rest.indexOf("\n");
      rest = end === -1 ? "" : rest.slice(end + 1);
    } else if (rest.startsWith("/*")) {
      const end = rest.indexOf("*/", 2);
      rest = end === -1 ? "" : rest.slice(end + 2);
    } else {
      break;
    }
  }
  let word = "";
  for (const char of rest) {
    if (!LETTER.test(char)) {
      break;
    }
    word += char;
  }
  return word;
}

/**
 * Counts the non-empty statements in `sql`, skipping `;` inside literals, quoted names, and
 * comments the way SQLite's tokenizer does, and returns where the first statement's `;` sits.
 * bun:sqlite exposes no `sqlite3_complete`. A trigger body's `;` counts as a split, which only
 * rejects `CREATE TRIGGER`, a write the read-only connection refuses anyway.
 */
export function scanStatements(sql: string): { readonly count: number; readonly end: number } {
  let count = 0;
  let end = sql.length;
  let open = false;
  let i = 0;
  while (i < sql.length) {
    const skipped = skipIgnored(sql, i);
    if (skipped !== undefined) {
      i = skipped;
    } else if (sql.charAt(i) === ";") {
      end = open && count === 1 ? i : end;
      open = false;
      i++;
    } else {
      count += open ? 0 : 1;
      open = true;
      i = skipToken(sql, i);
    }
  }
  return { count, end };
}

// An escaped `''` scans as two adjacent literals, which counts the same.
function skipToken(sql: string, i: number): number {
  const close = CLOSERS.get(sql.charAt(i));
  return close === undefined ? i + 1 : skipTo(sql, close, i + 1);
}

/** The index after the whitespace or comment at `i`, or `undefined` when none starts there. */
function skipIgnored(sql: string, i: number): number | undefined {
  if (/\s/.test(sql.charAt(i))) {
    return i + 1;
  }
  if (sql.startsWith("--", i)) {
    return skipTo(sql, "\n", i + 2);
  }
  if (sql.startsWith("/*", i)) {
    return skipTo(sql, "*/", i + 2);
  }
  return undefined;
}

const CLOSERS: ReadonlyMap<string, string> = new Map([
  ["'", "'"],
  ['"', '"'],
  ["`", "`"],
  ["[", "]"],
]);

function skipTo(sql: string, close: string, from: number): number {
  const at = sql.indexOf(close, from);
  return at === -1 ? sql.length : at + close.length;
}
