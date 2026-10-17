/** The current time. `Date` already holds whole milliseconds, which is what a row stores. */
export function now(): Date {
  return new Date();
}

/** Fixed-width `YYYY-MM-DDTHH:MM:SS.mmmZ`, so text order is time order. */
export function toDbTime(date: Date): string {
  return date.toISOString();
}
