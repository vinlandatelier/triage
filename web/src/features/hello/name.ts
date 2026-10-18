const NAME_MAX = 80;
export const NAME_ERROR = "name must be 1 to 80 characters";

const EDGE_SPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;

// Mirrors the api's `HelloName`: trim Unicode White_Space, then count code points, not UTF-16 units.
export function parseName(raw: string): string | undefined {
  const name = raw.replace(EDGE_SPACE, "");
  const length = [...name].length;
  // The server rejects control characters too: SQLite `length()` stops at NUL.
  if (/\p{Cc}/u.test(name)) return undefined;
  return length >= 1 && length <= NAME_MAX ? name : undefined;
}
