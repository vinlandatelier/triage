import { z } from "@hono/zod-openapi";
import { sql } from "drizzle-orm";
import { check, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const NAME_MAX = 80;

export const hello = sqliteTable(
  "hello",
  {
    id: text("id").primaryKey(),
    name: text("name").notNull(),
    createdAt: text("created_at").notNull(),
    updatedAt: text("updated_at").notNull(),
  },
  (table) => [
    check(
      "hello_name_length",
      sql`length(${table.name}) BETWEEN 1 AND ${sql.raw(String(NAME_MAX))}`,
    ),
  ],
);

export type HelloRow = typeof hello.$inferSelect;

// Unicode White_Space, which Rust's `trim` strips; JS `trim` also strips U+FEFF and keeps U+0085.
const EDGE_SPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;

/** Trims, then requires 1 to `NAME_MAX` characters and no control characters. */
export const HelloName = z
  // A missing name gets the same 422 as an empty one, as in the Rust build.
  .string({ error: (issue) => (issue.input === undefined ? "must not be empty" : undefined) })
  .overwrite((raw) => raw.replace(EDGE_SPACE, ""))
  .superRefine((name, ctx) => {
    const message = nameProblem(name);
    if (message !== undefined) {
      ctx.addIssue({ code: "custom", message });
    }
  });

function nameProblem(name: string): string | undefined {
  // Code points, not UTF-16 units, as the table's `length()` counts them.
  const length = [...name].length;
  if (length === 0) {
    return "must not be empty";
  }
  // SQLite's `length()` stops at NUL, so the table CHECK would see a different name.
  if (/\p{Cc}/u.test(name)) {
    return "must not contain control characters";
  }
  return length > NAME_MAX ? `must be at most ${NAME_MAX} characters` : undefined;
}

export const HelloSchema = z
  .object({
    id: z.string().openapi({ description: "A ULID." }),
    name: z.string(),
    // biome-ignore lint/style/useNamingConvention: the wire field is snake_case
    created_at: z.iso.datetime({ offset: true }),
  })
  .openapi("Hello");

export type Hello = z.infer<typeof HelloSchema>;
