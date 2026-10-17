import { expect, test } from "vitest";
import { isPath } from "../env";

test.each(["/", "/api", "/a//b"])("%j is a path", (v) => {
  expect(isPath(v)).toBe(true);
});

test.each(["", "api", "//x", "/\\x", "/\t/evil.com", "/\n/evil.com", "http://x"])(
  "%j is not a path",
  (v) => {
    expect(isPath(v)).toBe(false);
  },
);
