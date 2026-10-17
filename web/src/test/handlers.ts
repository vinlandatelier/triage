import {
  type DefaultBodyType,
  type HttpResponseResolver,
  http,
  type JsonBodyType,
  type PathParams,
} from "msw";
import { client } from "@/api/client.gen";
import type { Problem } from "@/api/types.gen";

type Method = Exclude<keyof typeof http, "all">;

/**
 * An MSW handler for `path` as written in the spec (`/health`), under the client's base URL.
 * The resolver may only return `Res` or a `Problem`, so a body of the wrong shape fails `tsc`.
 */
export function mock<
  Res extends JsonBodyType,
  Params extends PathParams<keyof Params> = PathParams,
  Body extends DefaultBodyType = DefaultBodyType,
>(method: Method, path: string, resolver: HttpResponseResolver<Params, Body, Res | Problem>) {
  return http[method]<Params, Body, Res | Problem>(
    `${client.getConfig().baseUrl}${path}`,
    resolver,
  );
}
