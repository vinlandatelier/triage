import { QueryClient } from "@tanstack/react-query";
import { client } from "@/api/client.gen";
import { toProblemError } from "@/api-config";
import { ProblemError } from "@/error";

// Registered here, not in api-config.ts: the generated client imports that file while it loads.
client.interceptors.error.use(toProblemError);

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: 1,
        staleTime: 30_000,
        throwOnError: (error) => error instanceof ProblemError && error.status >= 500,
      },
    },
  });
}
