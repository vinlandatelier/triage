import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Paths } from "../paths";
import { listen, lockDataDir, ServeError, serveUntil, signalQueue } from "./serve";

const temps: string[] = [];

afterEach(() => {
  for (const dir of temps.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function paths(): Paths {
  const dir = mkdtempSync(join(tmpdir(), "triage-serve-"));
  temps.push(dir);
  return { data: dir, logs: join(dir, "logs"), config: join(dir, "config") };
}

test("a second lock names the data dir, and a released lock can be taken again", () => {
  const dir = paths();
  const first = lockDataDir(dir);
  expect(() => lockDataDir(dir)).toThrow(
    new ServeError(`another triage instance is using ${dir.data}`),
  );
  first.release();
  lockDataDir(dir).release();
});

test("a port in use names the port and TRIAGE_PORT", () => {
  const held = listen(0, () => new Response("held"));
  try {
    let caught: unknown;
    try {
      listen(held.port ?? 0, () => new Response("second")).stop(true);
    } catch (err) {
      caught = err;
    }
    expect(caught).toBeInstanceOf(ServeError);
    expect(String(caught)).toContain(`port ${held.port} is already in use`);
    expect(String(caught)).toContain("TRIAGE_PORT");
  } finally {
    void held.stop(true);
  }
});

function slowServer(ms: number) {
  let entered: () => void = () => undefined;
  const started = new Promise<void>((resolve) => {
    entered = resolve;
  });
  const server = listen(0, async () => {
    entered();
    await Bun.sleep(ms);
    return new Response("done");
  });
  return { server, started };
}

test("a signal stops accepting and lets a slow request finish", async () => {
  const { server, started } = slowServer(300);
  const signals = signalQueue();
  const stopping = serveUntil(server, signals, 10_000);
  const request = fetch(`http://127.0.0.1:${server.port}/`);
  await started;
  signals.send();
  const response = await request;
  expect(await response.text()).toBe("done");
  expect(await stopping).toBe("drained");
  await expect(fetch(`http://127.0.0.1:${server.port}/`)).rejects.toThrow();
});

test("a signal sent before serving still stops the server", async () => {
  const server = listen(0, () => new Response("ok"));
  const signals = signalQueue();
  signals.send();
  expect(await serveUntil(server, signals, 10_000)).toBe("drained");
});

test("the drain window caps shutdown", async () => {
  const { server, started } = slowServer(60_000);
  const signals = signalQueue();
  const stopping = serveUntil(server, signals, 50);
  fetch(`http://127.0.0.1:${server.port}/`).catch(() => undefined);
  await started;
  const begin = performance.now();
  signals.send();
  expect(await stopping).toBe("cut");
  expect(performance.now() - begin).toBeGreaterThanOrEqual(49);
  void server.stop(true);
});

test("a second signal cuts the drain", async () => {
  const { server, started } = slowServer(60_000);
  const signals = signalQueue();
  const stopping = serveUntil(server, signals, 10_000);
  fetch(`http://127.0.0.1:${server.port}/`).catch(() => undefined);
  await started;
  signals.send();
  signals.send();
  expect(await stopping).toBe("cut");
  void server.stop(true);
});
