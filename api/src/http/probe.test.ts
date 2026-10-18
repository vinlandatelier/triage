import { expect, test } from "bun:test";
import { createServer, type Server } from "node:net";
import packageJson from "../../package.json" with { type: "json" };
import { createApp } from "./app";
import { probe } from "./probe";
import { listen } from "./serve";

async function rawServer(onConnect: (socket: import("node:net").Socket) => void): Promise<Server> {
  const server = createServer(onConnect);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return server;
}

function portOf(server: Server): number {
  const addr = server.address();
  return typeof addr === "object" && addr !== null ? addr.port : 0;
}

test("reads the version and uptime from the daemon", async () => {
  const server = listen(0, createApp({ startedAt: performance.now() }).fetch);
  try {
    expect(await probe(server.port ?? 0)).toEqual({
      status: "ok",
      version: packageJson.version,
      // biome-ignore lint/style/useNamingConvention: the wire field is snake_case
      uptime_s: 0,
    });
  } finally {
    void server.stop(true);
  }
});

test("a closed port is not running", async () => {
  const server = await rawServer(() => undefined);
  const port = portOf(server);
  server.close();
  expect(await probe(port)).toBeUndefined();
});

test("a silent listener times out as not running", async () => {
  const server = await rawServer(() => undefined);
  try {
    expect(await probe(portOf(server), 50)).toBeUndefined();
  } finally {
    server.close();
  }
});

test("an oversized response is not running", async () => {
  const server = await rawServer((socket) => {
    socket.write("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n");
    socket.end(" ".repeat(64 * 1024));
  });
  try {
    expect(await probe(portOf(server))).toBeUndefined();
  } finally {
    server.close();
  }
});

test("other services are not triage", async () => {
  for (const reply of [
    "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n",
    "HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\n<html>",
    "garbage",
    'HTTP/1.1 200 OK\r\ncontent-length: 44\r\n\r\n{"status":"down","version":"1","uptime_s":1}',
  ]) {
    const server = await rawServer((socket) => {
      socket.once("data", () => socket.end(reply));
    });
    try {
      expect(await probe(portOf(server)), reply).toBeUndefined();
    } finally {
      server.close();
    }
  }
});
