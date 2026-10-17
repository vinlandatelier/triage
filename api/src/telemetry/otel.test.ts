import { expect, test } from "bun:test";
import { SimpleSpanProcessor } from "@opentelemetry/sdk-trace";
import { startOtel } from "./otel";

test("a closed port warns once and never prints the password", async () => {
  const server = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response() });
  const port = server.port;
  await server.stop(true);
  const warnings: [string, Readonly<Record<string, unknown>>][] = [];
  // Simple, not batch: one export per span, so the second and third failures must stay quiet.
  const otel = startOtel(
    new URL(`http://user:hunter2@127.0.0.1:${port}/v1/traces`),
    (message, fields) => warnings.push([message, fields]),
    (exporter) => new SimpleSpanProcessor({ exporter }),
  );
  for (const name of ["first", "second", "third"]) {
    otel.endSpan(otel.startSpan(name, {}));
  }
  await otel.shutdown();
  expect(warnings).toHaveLength(1);
  const [message, fields] = warnings[0] ?? ["", {}];
  expect(message).toBe("cannot export traces; further OTLP errors are silent");
  expect(String(fields.endpoint)).toContain(String(port));
  expect(JSON.stringify(warnings)).not.toContain("hunter2");
}, 15_000);
