import { mutationOptions, queryOptions } from "@tanstack/react-query";
import type { InferResponseType } from "hono/client";
import { call, client } from "@/api-client";

export type Hello = InferResponseType<(typeof client.api.hello)[":id"]["$get"], 200>;

export const helloListKey = ["hello", "list"] as const;

export const helloListOptions = () =>
  queryOptions({
    queryKey: helloListKey,
    queryFn: ({ signal }) => call(client.api.hello.$get(undefined, { init: { signal } })),
  });

export const helloCreateOptions = () =>
  mutationOptions({
    mutationFn: (name: string) => call(client.api.hello.$post({ json: { name } })),
  });
