const NAME_MAX = 80;
export const NAME_ERROR = "name must be 1 to 80 characters";

// Mirrors `HelloName::parse`: trim, then count characters, not UTF-16 units.
export function parseName(raw: string): string | undefined {
  const name = raw.trim();
  const length = [...name].length;
  // The server rejects control characters too: SQLite `length()` stops at NUL.
  if (/\p{Cc}/u.test(name)) return undefined;
  return length >= 1 && length <= NAME_MAX ? name : undefined;
}
