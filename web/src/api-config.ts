import type { CreateClientConfig } from "@/api/client.gen";
import { NetworkError, ProblemError } from "@/error";

// Node's fetch, which the tests run on, rejects a relative URL such as the default `/`.
const baseUrl = new URL(import.meta.env.VITE_API_URL, window.location.origin).href;

export const createClientConfig: CreateClientConfig = (config) => ({ ...config, baseUrl });

// The client throws the parsed body of a failed response. fetch rejects with a TypeError when
// it gets no response at all; an abort keeps its own error. A 2xx lands here only when its body
// fails to parse, which is not a problem the server reported.
export function toProblemError(error: unknown, response: Response | undefined): unknown {
  if (response === undefined) return error instanceof TypeError ? new NetworkError(error) : error;
  if (response.ok) return error;
  return ProblemError.from(error, response);
}
