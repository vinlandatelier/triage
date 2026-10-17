import type { CreateClientConfig } from "@/api/client.gen";
import { ProblemError } from "@/error";

// Node's fetch, which the tests run on, rejects a relative URL such as the default `/api`.
const baseUrl = new URL(import.meta.env.VITE_API_URL, window.location.origin).href;

export const createClientConfig: CreateClientConfig = (config) => ({ ...config, baseUrl });

// The client throws the parsed body of a failed response. A fetch that never got a response
// keeps its own error.
export function toProblemError(error: unknown, response: Response | undefined): unknown {
  if (response === undefined) return error;
  return ProblemError.from(error, response);
}
