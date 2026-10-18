import type { AppType } from "@triage/api";
import { type ClientResponse, hc } from "hono/client";
import { NetworkError, ProblemError } from "@/error";

// Node's fetch, which the tests run on, rejects a relative URL such as the default `/`.
const baseUrl = new URL(import.meta.env.VITE_API_URL, window.location.origin).href;

export const client = hc<AppType>(baseUrl);

/**
 * The JSON body of a 2xx response. A failed response rejects with `ProblemError`, and a request
 * that got no response with `NetworkError`; an abort keeps its own error.
 */
export async function call<T>(request: Promise<ClientResponse<T, number, "json">>): Promise<T> {
  let response: ClientResponse<T, number, "json">;
  try {
    response = await request;
  } catch (error) {
    throw error instanceof TypeError ? new NetworkError(error) : error;
  }
  if (!response.ok) {
    // A proxy or a crashed daemon can answer with a body that is not JSON.
    const body: unknown = await response.json().catch(() => undefined);
    throw ProblemError.from(body, response);
  }
  return response.json();
}
