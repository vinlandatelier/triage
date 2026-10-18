import { expect, test } from "bun:test";
import packageJson from "../package.json" with { type: "json" };

test("--version prints the package version", async () => {
  const proc = Bun.spawn([process.execPath, "src/main.ts", "--version"], {
    cwd: `${import.meta.dir}/..`,
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  expect({ exitCode, stderr }).toEqual({ exitCode: 0, stderr: "" });
  expect(stdout.trim()).toBe(packageJson.version);
});
