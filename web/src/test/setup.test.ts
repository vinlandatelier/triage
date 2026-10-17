import { expect, test } from "vitest";
import { unhandledRequests } from "@/test/setup";

test("records a request that has no handler, even when the caller catches it", async () => {
  await expect(fetch("http://localhost/api/unmocked")).rejects.toThrow(TypeError);
  await fetch("http://localhost/api/caught").catch(() => {});

  expect(unhandledRequests.splice(0)).toEqual([
    "GET http://localhost/api/unmocked",
    "GET http://localhost/api/caught",
  ]);
});
