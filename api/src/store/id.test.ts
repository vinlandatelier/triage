import { expect, test } from "bun:test";
import { newId } from "./id";

test("newId is a 26-character ULID that sorts by time", () => {
  const id = newId(0x0123456789ab);
  expect(id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  expect(id.slice(0, 10)).toBe("014D2PF2DB");
  expect(newId(1) < newId(2)).toBe(true);
  expect(newId()).not.toBe(newId());
});
