import { QueryClient } from "@tanstack/react-query";
import { ProblemError } from "@/error";

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
