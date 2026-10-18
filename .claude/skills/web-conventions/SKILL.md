---
name: web-conventions
description: Use before writing or changing TypeScript or React in web/, above all a feature under web/src/features/ or anything that calls the API. Covers the typed hc client, call(), hand-written query options, MSW mocks, URL state, and the type rules, with features/hello as the worked example.
---

# Web conventions

`web/src/features/hello/` is the worked example. Copy its shape. `docs/ARCHITECTURE.md` holds the rules; where the two disagree, it wins. For TanStack Router and Query detail, the vendored `router-*` and `tanstack-query-best-practices` skills apply.

## The typed client

`src/api-client.ts` exports `client = hc<AppType>(…)`. `AppType` is a type-only import from `@triage/api`, so every path, param, body, and response type comes from the api's `createRoute` schemas. There is no generated code and no regenerate step.

| you need | it comes from |
|---|---|
| a response type | `InferResponseType<typeof client.api.hello.$get, 200>`, exported once per feature (`Hello` in `features/hello/queries.ts`) |
| `Problem`, `FieldError` | `import type { Problem } from "@triage/api"` |
| a call | `call(client.api.hello.$post({ json: { name } }))` |
| a query or mutation | the feature's `queries.ts` |

- Wrap every call in `call()`. It returns the 2xx body and rejects with `ProblemError` for a failed response or `NetworkError` for no response. Why: a bare client call resolves on a 4xx.
- `@triage/api` is types only. Never import a value from it. Why: web would bundle the daemon.
- After a route change in `api/`, `bun run typecheck` here: it emits the api types first, then fails wherever the web reads a changed shape. Fix those sites. Do not cast past them.
- Web never loads Bun types. `Bun.*` in web code fails `tsc`.

## Queries and mutations

Each feature writes its options once in `queries.ts`, over `call(client…)`. Components and loaders spread them; they never write a `queryKey` or `queryFn` inline.

```ts
// features/hello/queries.ts
export const helloListKey = ["hello", "list"] as const;

export const helloListOptions = () =>
  queryOptions({
    queryKey: helloListKey,
    queryFn: ({ signal }) => call(client.api.hello.$get(undefined, { init: { signal } })),
  });

export const helloCreateOptions = () =>
  mutationOptions({ mutationFn: (name: string) => call(client.api.hello.$post({ json: { name } })) });
```

```tsx
// routes/index.tsx: the route only wires a loader to the feature component.
loader: ({ context }) => context.queryClient.ensureQueryData(helloListOptions()),
component: HelloPage,

// features/hello/hello-page.tsx
const { data } = useSuspenseQuery(helloListOptions());

const create = useMutation({
  ...helloCreateOptions(),
  throwOnError: (error) => !(error instanceof ProblemError) || error.status >= 500,
  onSuccess: () => queryClient.invalidateQueries({ queryKey: helloListKey }),
});
create.mutate(parsed);
```

- Keys start with the feature name (`["hello", ...]`), so invalidating `["hello"]` clears the feature. Pass the query's `signal` to the call, so a cancelled query aborts its fetch.
- The loader primes the cache; the component reads it with `useSuspenseQuery`. A list failure reaches the route `errorComponent`.
- Show 4xx `ProblemError`s inline. Let 5xx and `NetworkError` throw to the `errorComponent`.
- Field errors map by `FieldError.field`, the JSON key: `error.errors?.find((e) => e.field === "name")`.
- Client-side checks mirror the api schema in a small module (`name.ts::parseName` mirrors `HelloName`). The server stays the authority.

## URL state via the Router

View state (filters, selected tab, page, open item) lives in the URL, not `useState` or a store. Validate search params on the route; the URL is user input.

```tsx
// Illustrative: no route reads search params yet.
const search = z.object({ q: z.string().optional(), page: z.number().int().min(1).default(1) });

export const Route = createFileRoute("/items")({
  validateSearch: search,
  loaderDeps: ({ search }) => search,
  loader: ({ context, deps }) => context.queryClient.ensureQueryData(itemListOptions({ query: deps })),
  component: ItemsPage,
});

// in the component
const { q, page } = Route.useSearch();
const navigate = Route.useNavigate();
navigate({ search: (prev) => ({ ...prev, page: prev.page + 1 }) });
```

- `useState` is for state that should not survive a reload or a shared link: an input's draft text, an open popover.
- No global client store. Server state is TanStack Query; view state is the URL.

## Type rules

**Discriminated unions over boolean flags.** Two booleans allow four states when there are three.

```ts
// no
type Props = { loading: boolean; error?: string; data?: Hello[] };
// yes
type State =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; hellos: Hello[] };
```

Switch on `kind`. A `default` branch that assigns the value to `never` makes `tsc` fail on a missed case.

**`satisfies` over `as`.** `satisfies` checks a value against a type and keeps the narrow type. `as` silences the checker.

```ts
const hello = { id: "id-1", name: "mars", created_at: "2026-10-17T00:00:00.000Z" } satisfies Hello;
```

- Test fixtures are typed values: `const hello = (name: string): Hello => ({ … })`, as in `hello-page.test.tsx`.
- `as` is allowed only to view an `unknown` object as a record after a runtime check, as `error.ts::isProblem` does. Never `as` a server response.
- Never `as unknown as T`.

**No `any`.** Use `unknown` and narrow it. `catch (error)` is `unknown`; pass it to `describeError` or check `instanceof ProblemError`. `ts-reset` (`src/reset.d.ts`) makes `JSON.parse` and `.json()` return `unknown`, so narrow those too.

**Zod only at trust boundaries.** The client types already describe API responses, and `tsc` fails on drift. Do not re-validate them with Zod.

| boundary | example |
|---|---|
| env vars | `web/env.ts` |
| URL search params | `validateSearch` above |
| `localStorage`, `sessionStorage` | the stored theme, any persisted draft |
| `postMessage`, clipboard, file uploads | anything a user or another origin supplies |

Form input is user input, but a small parser that mirrors the api schema (`parseName`) is enough. Reach for Zod when a form has several fields.

## Feature layout

```
web/src/features/<name>/
  <name>-page.tsx        the page component the route renders
  <name>-page.test.tsx   route-level tests through renderAt
  queries.ts             queryOptions and mutationOptions over call(client…), the feature's response types
  <helper>.ts            pure helpers such as parseName
```

- Route files in `src/routes/` hold a loader and a component reference, nothing else.
- Imports use `@/`. UI parts come from `@/components/ui/` (shadcn, ours to edit).
- Tests: copy `renderAt` from `hello-page.test.tsx`. Mock with `mock(client.api.hello.$get, …)` from `@/test/handlers`: the resolver is typed from the route's response schema, so a body of the wrong shape fails `tsc`. Drive forms with `userEvent`.

## When you are about to…

| about to | do instead / also |
|---|---|
| write a `type` for an API body | `InferResponseType` on the client endpoint, once in `queries.ts` |
| call `fetch` | `call(client…)` inside the feature's `queries.ts` |
| write a `queryKey` inline | use the key exported from `queries.ts` |
| add `useState` for a filter or tab | put it in the route's search params |
| add two booleans for one state | use a discriminated union |
| write `as SomeType` | use `satisfies`, a typed function, or a narrowing check |
| write `any` | use `unknown` and narrow |
| parse an API response with Zod | trust the client type |
| add a dependency | check the allowlist in `docs/ARCHITECTURE.md`; ask if absent |
| finish | `just check-web` |
