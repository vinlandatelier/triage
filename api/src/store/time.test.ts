import { expect, test } from "bun:test";
import { now, toDbTime, toRfc3339 } from "./time";

test("toDbTime writes fixed-width UTC with milliseconds", () => {
  expect(toDbTime(new Date(Date.UTC(2026, 0, 2, 3, 4, 5, 60)))).toBe("2026-01-02T03:04:05.060Z");
  expect(toDbTime(now())).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/);
});

test("toRfc3339 drops trailing sub-second zeros", () => {
  expect(toRfc3339("2026-01-02T03:04:00.000Z")).toBe("2026-01-02T03:04:00Z");
  expect(toRfc3339("2026-01-02T03:04:05.100Z")).toBe("2026-01-02T03:04:05.1Z");
  expect(toRfc3339("2026-01-02T03:04:05.060Z")).toBe("2026-01-02T03:04:05.06Z");
  expect(toRfc3339("2026-01-02T03:04:05.123Z")).toBe("2026-01-02T03:04:05.123Z");
});
