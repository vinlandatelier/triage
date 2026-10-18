import { desc, eq } from "drizzle-orm";
import type { Store, Transaction } from "../../store/open";
import { type HelloRow, hello } from "./model";

export function insertHello(tx: Transaction, row: HelloRow): void {
  tx.insert(hello).values(row).run();
}

export function listRecentHellos(store: Store, limit: number): HelloRow[] {
  return store.db
    .select()
    .from(hello)
    .orderBy(desc(hello.createdAt), desc(hello.id))
    .limit(limit)
    .all();
}

export function findHello(store: Store, id: string): HelloRow | undefined {
  return store.db.select().from(hello).where(eq(hello.id, id)).get() ?? undefined;
}
