---
name: api-conventions
description: Use before writing or changing TypeScript in api/, above all a feature under api/src/features/ (a route, a table, a field, an error, a log line). Lists the one way to write each thing, with the reason.
---

# API conventions

One way to write each thing in `api/`. Follow the rule; do not weigh alternatives. `docs/ARCHITECTURE.md` holds structure (layout, direction, boundaries) and wins where the two disagree. `api/biome.json` and `api/tsconfig.json` enforce every rule a tool can; this file holds the rest.

`api/src/features/hello/` is the worked example; copy its shape for a new feature. The walk below shows it file by file.

## Worked example: `features/hello/`

A feature is four files plus tests. Direction is `http.ts → service.ts → repo.ts`; `model.ts` is shared.

### `model.ts`: the table and the schemas

```ts
export const hello = sqliteTable("hello", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
}, (table) => [check("hello_name_length", sql`length(${table.name}) BETWEEN 1 AND ...`)]);
export type HelloRow = typeof hello.$inferSelect;

export const HelloName = z.string(...).trim().superRefine(...);   // one message per failure
export const HelloSchema = z.object({ id, name, created_at: z.iso.datetime(...) }).openapi("Hello");
export type Hello = z.infer<typeof HelloSchema>;
```

- Every table has `id TEXT PRIMARY KEY`, `created_at`, `updated_at`. Columns are camelCase in code, snake_case in SQL. Why: `schema.test.ts` checks every table.
- A CHECK in the table and the zod schema hold the same bounds. Why: the table is the last line; the schema gives the 422.
- The view schema is the wire shape, snake_case keys with a `biome-ignore` naming the reason. `.openapi("Name")` names it in the document. Why: the response shapes are the contract the web types read.
- After a change: `just db-generate`, then `just db-schema`. Name a new migration: `just db-generate --name <slug>`.

### `repo.ts`: Drizzle only

```ts
export function insertHello(tx: Transaction, row: HelloRow): void { tx.insert(hello).values(row).run(); }
export function listRecentHellos(store: Store, limit: number): HelloRow[] { ... .orderBy(desc(hello.createdAt), desc(hello.id)).limit(limit).all(); }
export function findHello(store: Store, id: string): HelloRow | undefined { ... .get() ?? undefined; }
```

- Writes take the `Transaction` that `store.write` passes; reads take the `Store`. Why: the service owns the transaction.
- Return `undefined` for a missing row, and turn every `null` column into `undefined`. Why: one absent value in app code.
- No hono, no zod, no `AppError`. Why: rule 3, and a repo does not know about HTTP.

### `service.ts`: work, spans, and logs

```ts
export function createHello(store: Store, name: string): HelloRow {
  return log.withSpan("hello.create", {}, (span) => {
    const at = toDbTime(now());
    const row: HelloRow = { id: newId(), name, createdAt: at, updatedAt: at };
    span.record({ "hello.id": row.id });
    store.write((tx) => insertHello(tx, row));
    log.debug("hello created", { "hello.id": row.id });
    return row;
  });
}
```

- One span per write, `<feature>.<verb>`. Record the new id on the span and log a debug event carrying it. Why: the JSONL file logs no span closes, so the event is how the id reaches it.
- A lookup returns `undefined`; the handler decides that it is a 404. Why: "missing" is not an error for every caller.
- Services take already-validated values. Why: validation happens once, at the boundary.

### `http.ts`: one `createRoute` per operation

```ts
const createHelloRoute = createRoute({
  method: "post", path: "/api/hello", operationId: "hello_create", tags: ["hello"], summary, description,
  request: { body: { required: true, content: { "application/json": { schema: CreateHelloSchema } } } },
  responses: { 201: { description: "The new hello", content: { "application/json": { schema: HelloSchema } } } },
});

export function createHelloRoutes(deps: HelloDeps) {
  return createRouter()
    .openapi(createHelloRoute, (c) => c.json(helloView(createHello(deps.store, c.req.valid("json").name)), 201))
    .openapi(listHellosRoute, ...)
    .openapi(getHelloRoute, ...);
}
```

- Build on `createRouter()` from `http/router.ts` and chain every `.openapi(...)`. Why: the hook makes the 422, and the chain is what carries the types to `AppType`.
- `operationId` is `<feature>_<verb>`; paths are the real `/api/...` path with `{param}`. The `Problem` default response is added for you. Why: the document reads the same as the Rust one.
- Read input only through `c.req.valid(...)`. Throw `AppError` for 4xx (`new AppError("not_found", \`no hello ${id}\`)`). Why: `onError` renders it.
- Map rows to views here (`helloView`), timestamps through `toRfc3339`. Why: the wire shape is HTTP's concern.
- `createApp` mounts it: `.route("/", createHelloRoutes(deps))`, chained. Then `cd web && bun run typecheck`: the web reads the new types.

### `hello.test.ts`: the HTTP contract

- `openTempStore()` per test, `createApp({ startedAt, store })`, then `testClient(app)`; dispose in `afterEach`. Why: a fresh file per test, real Drizzle.
- Snapshot bodies with `id`, `created_at`, `updated_at`, and `instance` redacted. Why: the snapshot pins the shape, not the values.
- For a status the route does not declare (404, 422), assert with `expect<number>(response.status)`. Why: the client types only the declared statuses.

## Errors

- Throw inside the app. Handlers throw `AppError`; one `app.onError` maps it to problem+json. No neverthrow or other Result type. Why: Hono's error model is throw + `onError`, and Biome has no lint to force a Result to be consumed.
- One `AppError extends Error` with a `code` string-literal union and a `status`. Subclass only when a handler needs `instanceof`. Why: a union on `code` keeps switches exhaustive.
- Wrap and rethrow with `new AppError(msg, { cause: err })`. Never drop the original. Why: `cause` keeps the original error and its stack.
- `catch (err)` is `unknown`; narrow with `instanceof`. Never throw a string. Why: a non-`Error` throw has no stack.
- Clients get problem+json only. Stack traces never leave the process; an unknown error becomes a generic 500 and is logged. Why: OWASP, internals stay internal.

## Types

- `type` by default. `interface` only for a shape another declaration `extends`. Why: no silent declaration merging, and a `type` can be a union.
- Never `enum`. Use an `as const` object and `(typeof X)[keyof typeof X]`. Why: `enum` emits runtime code and is nominal; `erasableSyntaxOnly` and `noEnum` reject it.
- `satisfies` on config and lookup literals instead of an annotation. Why: it checks the shape without widening the type.
- Every `switch` over a union ends `default: { const _: never = x; throw ... }`. Why: the compiler fails when a member is added.
- `undefined` means absent. `null` appears only where the outside world produces it (nullable columns, JSON); `repo.ts` converts it. Why: one absent value in app code.
- `prop?: T` for optional fields. `prop?: T | undefined` only when callers really pass `undefined`. Why: `exactOptionalPropertyTypes` keeps absent and set-to-undefined apart.
- Index reads include `undefined`; check them. The `!` operator is banned. Why: `noUncheckedIndexedAccess` and `noNonNullAssertion` make the miss visible.
- `readonly` on every property that is not reassigned, `as const` on literal tables, no parameter reassignment. Copy with spread only when returning internal state. Why: data does not change behind a caller's back.

## Modules

- Named exports only. A default export is allowed only through a per-file Biome override where a tool requires it. Why: one canonical name per thing, found by search.
- No `index.ts` barrels and no `export *`. Why: barrels load unrelated modules and hide the import graph.
- `import type` and `export type` for types; `node:` and `bun:` prefixes for built-ins; Biome organizes imports. Why: pairs with `verbatimModuleSyntax`.
- Functions and plain objects. A class only for an `Error` subclass or a resource with a lifecycle. No classes of static methods. Why: Hono advises no controllers, and a namespace class is a module with extra steps.
- A feature's `createRoutes(deps)` takes one typed `deps` object. Per-request values live in Hono `Variables`. No DI container. Why: Hono's typed context is the injection point.
- Files are kebab-case. Types are PascalCase, functions and variables camelCase, module constants CONSTANT_CASE. One concept per file; `x.test.ts` sits beside `x.ts`. Why: Biome enforces the names, so nobody chooses.

## Async

- `async`/`await` only. Every promise is awaited, returned, or `void`ed. `Promise.all` for independent work. A cancellable function takes an `AbortSignal` option. Why: an unhandled promise swallows its error; `noFloatingPromises`, `noMisusedPromises`, and `useAwait` catch most cases.

## Validation

- zod at every trust boundary: env (parsed once at startup), HTTP input, external payloads. Internal calls trust the types. Why: OWASP, validate where data enters.
- Types come from schemas through `z.infer`, `z.input`, or `z.output`. Never hand-write a type a schema defines. Why: one source of truth.
- Request and view schemas are hand-written zod in `model.ts`, next to the table (`drizzle-zod` is not allowlisted). Rows read from our own database are not validated again. Why: the table's constraints already hold.
- `safeParse` at boundaries. `parse` only where a failure is a bug. Why: expected failures need no try/catch.
- `z.strictObject` at boundaries, except a request body the Rust build accepted with extra fields (`CreateHello`), which stays `z.object` until the port is done. A dictionary keyed by user input is a `Map` or `Object.create(null)`. Why: unknown fields are rejected and prototype pollution has no target; the contract does not change during the port.

## Security

- `Bun.serve({ hostname: "127.0.0.1" })` always. Why: Bun binds `0.0.0.0` by default, which exposes the daemon to the LAN.
- Hono `bodyLimit` caps bodies; `Bun.serve`'s `maxRequestBodySize` sits above it as a backstop. Why: an unbounded body is a denial of service, and Bun's own 413 has no problem body.
- No `eval`, `new Function`, or `node:vm` on any input. Why: remote code execution.
- Never build a file path from user input. Why: path traversal.
- Commit `bun.lock`, install with `--frozen-lockfile` in CI, keep `bun audit --audit-level=high` green, keep `minimumReleaseAge` and exact versions in `bunfig.toml`, add no `trustedDependencies` without review. Why: bun does not freeze the lockfile in CI by default.

## Tests

- In-process `app.request()` or `testClient`, a fresh SQLite file per test, real Drizzle, no database mocks. Snapshots stay small, committed, and reviewed. Why: mocking the database tests the mock.
- `bun test`, not vitest. Why: vitest runs on Node and cannot import `bun:sqlite`.

## Storage

- Write through `store.write((tx) => …)`, never `store.db.transaction` or a bare write. Why: `write` takes the lock with `BEGIN IMMEDIATE`, so `busy_timeout` can wait for it.
- A new table goes in the feature's `model.ts`; run `just db-generate`, then `just db-schema`. Never edit an applied migration. Why: the migrator refuses a file whose hash changed.
- Ids come from `newId()`, timestamps from `toDbTime(now())`. Why: ULID text and fixed-width UTC sort by time as text.

## Logging

- One `const log = logger("<module>")` per module, at module level; call `log.info("message", { fields })`. Never pass a logger as a parameter or import pino. Why: the module logger reads the current span, so every line gets `request_id` without plumbing.
- `log.withSpan(name, fields, fn)` is the only way to scope or time work. Name spans `<feature>.<verb>` (`hello.create`); put ids in fields (`requestId`); add late values with `span.record({ status })`. Never time work with `performance.now()` by hand. Why: one helper keeps the line shape, `req=`, close durations, and OTLP in one place.
- Field keys are camelCase; the sinks write them snake_case. The message is a fixed string; values go in fields, never interpolated. Why: agents grep the file by message and key.
- Log an error as the field `error: err`, after wrapping it with `cause`. Why: both sinks print the whole `cause` chain only from an `Error` value.
- Never log a request body, a header map, or a `Secret`'s `expose()`. Keys with a segment `authorization`, `cookie`, `password`, `secret`, or `token` print `[redacted]` as a backstop, not a licence. Why: OWASP logging.
- Levels: `error` needs a person, `warn` is degraded but handled, `info` is lifecycle, `debug` is detail for agents (the file's default), `trace` is off. Why: a level means one thing everywhere.

## Sources

- Hono: [exception](https://hono.dev/docs/api/exception), [best practices](https://hono.dev/docs/guides/best-practices), [app](https://hono.dev/docs/api/hono), [validation](https://hono.dev/docs/guides/validation), [body-limit](https://hono.dev/docs/middleware/builtin/body-limit), [testing](https://hono.dev/docs/guides/testing)
- [neverthrow README](https://github.com/supermacro/neverthrow)
- [Google TypeScript style guide](https://google.github.io/styleguide/tsguide.html)
- [MDN Error.cause](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Error/cause)
- TypeScript: [useUnknownInCatchVariables](https://www.typescriptlang.org/tsconfig/useUnknownInCatchVariables.html), [5.8 release notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-8.html), [4.9 release notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-4-9.html), [narrowing](https://www.typescriptlang.org/docs/handbook/2/narrowing.html), [exactOptionalPropertyTypes](https://www.typescriptlang.org/tsconfig/exactOptionalPropertyTypes.html), [noUncheckedIndexedAccess](https://www.typescriptlang.org/tsconfig/noUncheckedIndexedAccess.html)
- [Total TypeScript: type vs interface](https://www.totaltypescript.com/type-vs-interface-which-should-you-use), [TSConfig cheat sheet](https://www.totaltypescript.com/tsconfig-cheat-sheet)
- Biome: [noEnum](https://biomejs.dev/linter/rules/no-enum/), [noDefaultExport](https://biomejs.dev/linter/rules/no-default-export/), [noBarrelFile](https://biomejs.dev/linter/rules/no-barrel-file/), [useImportType](https://biomejs.dev/linter/rules/use-import-type/), [noFloatingPromises](https://biomejs.dev/linter/rules/no-floating-promises/), [noParameterAssign](https://biomejs.dev/linter/rules/no-parameter-assign/), [useNamingConvention](https://biomejs.dev/linter/rules/use-naming-convention/), [domains](https://biomejs.dev/linter/domains/)
- [zod basics](https://zod.dev/basics), [drizzle-zod](https://orm.drizzle.team/docs/zod)
- OWASP: [Node.js](https://cheatsheetseries.owasp.org/cheatsheets/Nodejs_Security_Cheat_Sheet.html), [input validation](https://cheatsheetseries.owasp.org/cheatsheets/Input_Validation_Cheat_Sheet.html), [prototype pollution](https://cheatsheetseries.owasp.org/cheatsheets/Prototype_Pollution_Prevention_Cheat_Sheet.html), [path traversal](https://community.owasp.org/attacks/Path_Traversal), [logging](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html)
- Bun: [HTTP](https://bun.com/docs/api/http), [install](https://bun.com/docs/pm/cli/install), [audit](https://bun.com/docs/pm/cli/audit)
- [pino redaction](https://github.com/pinojs/pino/blob/main/docs/redaction.md)
