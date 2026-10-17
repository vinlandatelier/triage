---
name: web-conventions
description: Use before writing or changing TypeScript or React in web/, above all a feature under web/src/features/ or anything that calls the API. Covers the generated client in src/api/, the regenerate flow, query factories, URL state, and the type rules, with features/hello as the worked example.
---

# Web conventions

`web/src/features/hello/` is the worked example. Copy its shape. `docs/ARCHITECTURE.md` holds the rules; where the two disagree, it wins. For TanStack Router and Query detail, the vendored `router-*` and `tanstack-query-best-practices` skills apply.

## Never edit `src/api/`

`web/src/api/` is hey-api output for `crates/triage/openapi.json`. A hand edit is lost on the next regenerate and fails `just gen-api-check`. Change the Rust side, then regenerate.

| you need | it comes from |
|---|---|
| a request or response type | `@/api` (`Hello`, `CreateHello`, `HelloListResponse`, `Problem`, `FieldError`) |
| a query | `helloListOptions()` from `@/api/@tanstack/react-query.gen` |
| a mutation | `helloCreateMutation()` from the same file |
| a cache key to invalidate | `helloListQueryKey()` from the same file |
| a raw call outside React | `helloList()` from `@/api/sdk.gen`; rare |

Names come from the Rust `operation_id`: `hello_create` becomes `helloCreate`, `helloCreateMutation`. A missing name means the Rust operation is missing or misnamed.

## Regenerate flow

After any change to a handler, view struct, or `#[utoipa::path]` in `crates/triage/src/`:

```sh
just openapi    # crates/triage/openapi.json from the Rust code
just gen-api    # web/src/api/ from openapi.json
bun run typecheck
```

- Commit `openapi.json` and `web/src/api/` with the Rust change.
- A changed response type now fails `tsc` wherever the web code reads it. Fix those sites. Do not cast past them.
- Test fixtures typed as `Hello` fail too. Update them.

## Queries and mutations

Spread the generated factory. Never write a `queryKey` or `queryFn` by hand.

```tsx
// routes/index.tsx: the route only wires a loader to the feature component.
loader: ({ context }) => context.queryClient.ensureQueryData(helloListOptions()),
component: HelloPage,

// features/hello/hello-page.tsx
const { data } = useSuspenseQuery(helloListOptions());

const create = useMutation({
  ...helloCreateMutation(),
  throwOnError: (error) => !(error instanceof ProblemError) || error.status >= 500,
  onSuccess: () => queryClient.invalidateQueries({ queryKey: helloListQueryKey() }),
});
create.mutate({ body: { name: parsed } });
```

- The loader primes the cache; the component reads it with `useSuspenseQuery`. A list failure reaches the route `errorComponent`.
- Failed calls reject with `ProblemError` (`status`, `detail`, `errors`) or `NetworkError`. Show 4xx inline. Let 5xx and `NetworkError` throw to the `errorComponent`.
- Field errors map by `FieldError.field`, the JSON key: `error.errors?.find((e) => e.field === "name")`.
- Client-side checks mirror the Rust `parse` in a small module (`name.ts::parseName` mirrors `HelloName::parse`). The server stays the authority.

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

**Zod only at trust boundaries.** The generated types already describe API responses, and CI fails on drift. Do not re-validate them with Zod.

| boundary | example |
|---|---|
| env vars | `web/env.ts` |
| URL search params | `validateSearch` above |
| `localStorage`, `sessionStorage` | the stored theme, any persisted draft |
| `postMessage`, clipboard, file uploads | anything a user or another origin supplies |

Form input is user input, but a small parser that mirrors the Rust one (`parseName`) is enough. Reach for Zod when a form has several fields.

## Feature layout

```
web/src/features/<name>/
  <name>-page.tsx        the page component the route renders
  <name>-page.test.tsx   route-level tests through renderAt
  <helper>.ts            pure helpers such as parseName
```

- Route files in `src/routes/` hold a loader and a component reference, nothing else.
- Imports use `@/`. UI parts come from `@/components/ui/` (shadcn, ours to edit).
- Tests: copy `renderAt` from `hello-page.test.tsx`. Mock with `mock<HelloListResponse>("get", "/api/hello", …)` from `@/test/handlers`, so a body of the wrong shape fails `tsc`. Drive forms with `userEvent`.

## When you are about to…

| about to | do instead / also |
|---|---|
| edit a file in `src/api/` | change Rust, `just openapi`, `just gen-api` |
| write a `type` for an API body | import it from `@/api` |
| call `fetch` | use the generated `*Options()` or `*Mutation()` |
| write a `queryKey` array | use `<op>QueryKey()` |
| add `useState` for a filter or tab | put it in the route's search params |
| add two booleans for one state | use a discriminated union |
| write `as SomeType` | use `satisfies`, a typed function, or a narrowing check |
| write `any` | use `unknown` and narrow |
| parse an API response with Zod | trust the generated type |
| add a dependency | check the allowlist in `docs/ARCHITECTURE.md`; ask if absent |
| finish | `just check-web`, and `just gen-api-check` if the contract changed |
