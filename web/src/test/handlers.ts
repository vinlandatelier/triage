import {
  type DefaultBodyType,
  type HttpResponseResolver,
  http,
  type JsonBodyType,
  type PathParams,
} from "msw";
import { client } from "@/api/client.gen";
import type { Problem } from "@/api/types.gen";

// biome-ignore lint/suspicious/noExplicitAny: matches every generated SDK function signature.
type Operation = (options: any) => Promise<unknown>;
type Options<Op extends Operation> = NonNullable<Parameters<Op>[0]>;
type OrDefault<T, D> = [NonNullable<T>] extends [never] ? D : NonNullable<T>;

type ResponseOf<Op extends Operation> =
  Awaited<ReturnType<Op>> extends { data?: infer D }
    ? Extract<NonNullable<D>, JsonBodyType>
    : never;
type ParamsOf<Op extends Operation> = Extract<
  OrDefault<Options<Op>["path"], PathParams>,
  PathParams
>;
type BodyOf<Op extends Operation> = Extract<
  OrDefault<Options<Op>["body"], DefaultBodyType>,
  DefaultBodyType
>;

const methods = ["get", "post", "put", "patch", "delete", "head", "options"] as const;
type Method = (typeof methods)[number];

// The generated types drop `url` and the method, so call the SDK function on a stub client
// that records them instead of sending a request.
function routeOf(op: Operation): { method: Method; path: string } {
  let route: { method: Method; path: string } | undefined;
  const stub = Object.fromEntries(
    methods.map((method) => [
      method,
      ({ url }: { url: string }) => {
        route = { method, path: url.replace(/\{(\w+)\}/g, ":$1") };
        return Promise.resolve();
      },
    ]),
  );
  void op({ client: stub });
  if (route === undefined) throw new Error(`${op.name} did not call the client`);
  return route;
}

/**
 * An MSW handler for a generated SDK function's endpoint, under the client's base URL.
 * The resolver may only return that operation's response or a `Problem`, so a body of the wrong
 * shape fails `tsc`.
 */
export function mock<Op extends Operation>(
  op: Op,
  resolver: HttpResponseResolver<ParamsOf<Op>, BodyOf<Op>, ResponseOf<Op> | Problem>,
) {
  const { method, path } = routeOf(op);
  return http[method]<ParamsOf<Op>, BodyOf<Op>, ResponseOf<Op> | Problem>(
    `${client.getConfig().baseUrl}${path}`,
    resolver,
  );
}
