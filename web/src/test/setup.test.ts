import { expect, test, vi } from "vitest";
import { unhandledRequests } from "@/test/setup";

test("rejects and records a request that has no handler, even when the caller catches it", async () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  await expect(fetch("http://localhost/api/unmocked")).rejects.toMatchObject({
    message: "fetch failed",
    cause: { type: "error" },
  });
  await fetch("http://localhost/api/caught").catch(() => {});

  expect(unhandledRequests.splice(0)).toEqual([
    "GET http://localhost/api/unmocked",
    "GET http://localhost/api/caught",
  ]);
  consoleError.mockRestore();
});
