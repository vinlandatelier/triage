import type { z } from "zod";
import { logger } from "../telemetry";
import { HealthSchema } from "./app";

export type Health = z.infer<typeof HealthSchema>;

const TIMEOUT_MS = 2000;
// A real health body is under 100 bytes; the cap stops a foreign listener from filling memory.
const MAX_RESPONSE = 16 * 1024;

const log = logger("http");

/** Asks the daemon on `127.0.0.1:port` for its health. `undefined` means nothing healthy answered. */
export async function probe(
  port: number,
  timeoutMs: number = TIMEOUT_MS,
): Promise<Health | undefined> {
  const url = `http://127.0.0.1:${port}/health`;
  try {
    const response = await fetch(url, {
      signal: AbortSignal.timeout(timeoutMs),
      redirect: "manual",
    });
    if (response.status !== 200) {
      throw new Error(`status ${response.status}`);
    }
    const parsed = HealthSchema.safeParse(JSON.parse(await cappedText(response)));
    if (!parsed.success) {
      throw new Error("not a triage health body", { cause: parsed.error });
    }
    return parsed.data;
  } catch (err) {
    log.debug("health probe failed", { url, error: err });
    return undefined;
  }
}

async function cappedText(response: Response): Promise<string> {
  const reader = response.body?.getReader();
  if (reader === undefined) {
    return "";
  }
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const chunk = await reader.read();
    if (chunk.done) {
      break;
    }
    size += chunk.value.byteLength;
    if (size > MAX_RESPONSE) {
      await reader.cancel();
      throw new Error("response is too large");
    }
    chunks.push(chunk.value);
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks));
}
