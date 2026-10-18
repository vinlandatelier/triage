/** The current time. `Date` already holds whole milliseconds, which is what a row stores. */
export function now(): Date {
  return new Date();
}

/** Fixed-width `YYYY-MM-DDTHH:MM:SS.mmmZ`, so text order is time order. */
export function toDbTime(date: Date): string {
  return date.toISOString();
}

/** RFC 3339 for a `toDbTime` value, trailing sub-second zeros dropped as the Rust build did. */
export function toRfc3339(dbTime: string): string {
  return dbTime.replace(/\.000Z$/, "Z").replace(/(\.\d*?)0+Z$/, "$1Z");
}
