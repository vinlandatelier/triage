import type { Database } from "bun:sqlite";
import schemaSql from "../../schema.sql" with { type: "text" };
import { reason, StoreError } from "./error";

/** `schema.sql` as of this build, so `triage db check` needs no source tree. */
export const SCHEMA: string = schemaSql;

/**
 * The schema as `;`-terminated `CREATE` statements separated by blank lines, sorted by type then
 * name because `.schema` order is not stable.
 */
export function dumpSchema(client: Database): string {
  try {
    // `ANALYZE` and `PRAGMA optimize` create `sqlite_stat*` tables at runtime.
    const rows = client
      .query<{ sql: string }, []>(
        "SELECT sql FROM sqlite_schema WHERE sql NOT NULL AND name NOT GLOB 'sqlite_stat*' ORDER BY type DESC, name",
      )
      .all();
    return rows.map((row) => `${row.sql};\n`).join("\n");
  } catch (err) {
    throw new StoreError(`cannot read the schema: ${reason(err)}`, { cause: err });
  }
}
