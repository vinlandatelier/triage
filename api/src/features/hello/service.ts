import { newId } from "../../store/id";
import type { Store } from "../../store/open";
import { now, toDbTime } from "../../store/time";
import { logger } from "../../telemetry";
import type { HelloRow } from "./model";
import { findHello, insertHello, listRecentHellos } from "./repo";

const LIST_LIMIT = 20;

const log = logger("hello");

/** `name` has already passed `HelloName`. */
export function createHello(store: Store, name: string): HelloRow {
  return log.withSpan("hello.create", {}, (span) => {
    const at = toDbTime(now());
    const row: HelloRow = { id: newId(), name, createdAt: at, updatedAt: at };
    span.record({ "hello.id": row.id });
    store.write((tx) => insertHello(tx, row));
    // The JSONL file logs no span closes, so this event is where `hello.id` reaches it.
    log.debug("hello created", { "hello.id": row.id });
    return row;
  });
}

/** The 20 newest, newest first. */
export function listHellos(store: Store): HelloRow[] {
  return listRecentHellos(store, LIST_LIMIT);
}

export function getHello(store: Store, id: string): HelloRow | undefined {
  return findHello(store, id);
}
