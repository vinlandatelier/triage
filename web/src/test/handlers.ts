import type { Problem } from "@triage/api";
import type { ClientRequestOptions, InferRequestType, InferResponseType } from "hono/client";
import {
  type DefaultBodyType,
  type HttpResponseResolver,
  http,
  type JsonBodyType,
  type PathParams,
} from "msw";

// biome-ignore lint/suspicious/noExplicitAny: matches every client endpoint's argument type.
type Endpoint = (args: any, options?: ClientRequestOptions) => Promise<unknown>;
type Args<E extends Endpoint> = InferRequestType<E>;

type ResponseOf<E extends Endpoint> = Extract<InferResponseType<E>, JsonBodyType>;
type ParamsOf<E extends Endpoint> =
  Args<E> extends { param: infer P extends PathParams } ? P : PathParams;
type BodyOf<E extends Endpoint> =
  Args<E> extends { json: infer J extends DefaultBodyType } ? J : DefaultBodyType;

const methods = ["get", "post", "put", "patch", "delete"] as const;
type Method = (typeof methods)[number];
const isMethod = (value: string): value is Method => methods.some((m) => m === value);

// Calls the endpoint with a fetch that records the request instead of sending it. The client
// calls fetch before its first `await`, so this stays synchronous. With no `param`, the URL keeps
// `:id`, which is MSW's path syntax too.
function routeOf(endpoint: Endpoint): { method: Method; url: string } {
  let route: { method: Method; url: string } | undefined;
  void endpoint(undefined, {
    fetch: (input: RequestInfo | URL, init?: RequestInit) => {
      const method = (init?.method ?? "GET").toLowerCase();
      if (isMethod(method)) route = { method, url: String(input) };
      return Promise.resolve(new Response());
    },
  });
  if (route === undefined) throw new Error("the endpoint did not call fetch with a known method");
  return route;
}

/**
 * An MSW handler for a typed-client endpoint such as `client.api.hello.$get`. The resolver may
 * only return that route's response or a `Problem`, so a body of the wrong shape fails `tsc`.
 */
export function mock<E extends Endpoint>(
  endpoint: E,
  resolver: HttpResponseResolver<ParamsOf<E>, BodyOf<E>, ResponseOf<E> | Problem>,
) {
  const { method, url } = routeOf(endpoint);
  return http[method]<ParamsOf<E>, BodyOf<E>, ResponseOf<E> | Problem>(url, resolver);
}
