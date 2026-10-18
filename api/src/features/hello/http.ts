import { createRoute, z } from "@hono/zod-openapi";
import { AppError } from "../../error";
import { createRouter } from "../../http/router";
import type { Store } from "../../store/open";
import { toRfc3339 } from "../../store/time";
import { type Hello, HelloName, type HelloRow, HelloSchema, NAME_MAX } from "./model";
import { createHello, getHello, listHellos } from "./service";

export type HelloDeps = { readonly store: Store };

// Not strict: the Rust build ignored unknown fields, and the contract stays.
const CreateHelloSchema = z.object({ name: HelloName }).openapi("CreateHello");

const ULID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/i;

const createHelloRoute = createRoute({
  method: "post",
  path: "/api/hello",
  operationId: "hello_create",
  tags: ["hello"],
  summary: "Create a hello",
  description: `Trims the name, which must then be 1 to ${NAME_MAX} characters.`,
  request: {
    body: { required: true, content: { "application/json": { schema: CreateHelloSchema } } },
  },
  responses: {
    201: {
      description: "The new hello",
      content: { "application/json": { schema: HelloSchema } },
    },
  },
});

const listHellosRoute = createRoute({
  method: "get",
  path: "/api/hello",
  operationId: "hello_list",
  tags: ["hello"],
  summary: "List hellos",
  description: "The 20 newest, newest first.",
  responses: {
    200: {
      description: "The newest hellos",
      content: { "application/json": { schema: z.array(HelloSchema) } },
    },
  },
});

const getHelloRoute = createRoute({
  method: "get",
  path: "/api/hello/{id}",
  operationId: "hello_get",
  tags: ["hello"],
  summary: "Get a hello",
  request: {
    params: z.object({
      id: z
        .string()
        .openapi({ param: { name: "id", in: "path" }, description: "The hello's ULID" }),
    }),
  },
  responses: {
    200: {
      description: "The hello",
      content: { "application/json": { schema: HelloSchema } },
    },
  },
});

export function createHelloRoutes(deps: HelloDeps) {
  return createRouter()
    .openapi(createHelloRoute, (c) => {
      const { name } = c.req.valid("json");
      return c.json(helloView(createHello(deps.store, name)), 201);
    })
    .openapi(listHellosRoute, (c) => c.json(listHellos(deps.store).map(helloView), 200))
    .openapi(getHelloRoute, (c) => {
      const { id } = c.req.valid("param");
      // A malformed id names no row, so it gets the same 404 as a missing one. ULIDs are
      // case-insensitive; stored ones are uppercase.
      const row = ULID.test(id) ? getHello(deps.store, id.toUpperCase()) : undefined;
      if (row === undefined) {
        throw new AppError("not_found", `no hello ${id}`);
      }
      return c.json(helloView(row), 200);
    });
}

function helloView(row: HelloRow): Hello {
  // biome-ignore lint/style/useNamingConvention: the wire field is snake_case
  return { id: row.id, name: row.name, created_at: toRfc3339(row.createdAt) };
}
