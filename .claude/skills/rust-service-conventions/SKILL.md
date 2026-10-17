---
name: rust-service-conventions
description: Use before writing or changing Rust in crates/triage, above all a feature under src/features/ (an endpoint, a table, a field, an error, a span). Walks features/hello file by file and lists the regenerate commands each change needs.
---

# Rust service conventions

`crates/triage/src/features/hello/` is the worked example. Copy its shape; do not invent a new one. `docs/ARCHITECTURE.md` holds the rules. This skill shows them in code. Where the two disagree, `docs/ARCHITECTURE.md` wins.

## Layout and direction

```
features/<name>/
  mod.rs      module list, one doc line
  model.rs    validated newtypes and domain structs, no I/O
  repo.rs     every SQL statement, nothing else
  service.rs  transactions, spans, business rules
  http.rs     routes, request and view types, utoipa annotations
```

- Calls go `http → service → repo`. `model` is shared by all three.
- `http.rs` never names `sqlx`. `repo.rs` never names `axum`. `just check-layout` greps for both.
- A feature dir holds exactly those five files. A sixth file fails `just check-layout`. Put helpers in the file that owns the layer.
- `mod.rs` keeps `repo` private: `mod repo;`, `pub mod http; pub mod model; pub mod service;`.
- Register the feature once in `features/mod.rs`: `.merge(<name>::http::routes())`.

## hello, file by file

### `migrations/0001_hello.sql`

```sql
CREATE TABLE hello (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
```

- Every table is `STRICT`, snake_case, with `id TEXT PRIMARY KEY`, `created_at`, `updated_at`. `schema::schema_conventions` fails otherwise.
- Migrations are forward-only. Never edit an applied file: startup fails with `checksum`. Add `NNNN_<what>.sql` instead.
- Put the bound in a `CHECK` too. The model then decodes without re-validating.

### `model.rs`

- `HelloId(Ulid)`: one id newtype per feature. It implements `Display`, `FromStr`, and `sqlx::{Type, Encode, Decode}` as TEXT. Copy the four impls and rename.
- `HelloName::parse(&str) -> Result<Self>`: trim, then check bounds in `chars()`, not bytes. A failure is `AppError::Validation(vec![FieldError { field: "name".into(), message }])`. The `field` is the JSON key the client sent.
- `#[sqlx(transparent)]` on value newtypes, so `repo.rs` binds and decodes them directly.
- `Hello` is the stored row. `NewHello` is what a caller supplies. The service fills id and timestamps.
- Unit tests for parsing live here under `#[cfg(test)]`.

### `repo.rs`

- One `pub async fn` per statement: `insert(&mut SqliteConnection, …)` for writes, `get(&SqlitePool, …)` and `list_recent(&SqlitePool, …)` for reads. Writes take a connection so the service owns the transaction.
- Only `sqlx::query!` / `query_as!`. Never `sqlx::query(...)` with a string: the macros check SQL at compile time against `.sqlx/`.
- Select typed columns with overrides: `id AS "id: HelloId"`, `created_at AS "created_at: OffsetDateTime"`.
- Write timestamps through `timestamp()`, the fixed-width `YYYY-MM-DDTHH:MM:SS.mmmZ` format, so text order is time order. Copy `TIMESTAMP` and `timestamp()` into a new feature's `repo.rs`.
- Return `Result<Option<T>>` for a lookup. The service turns `None` into `NotFound`.

### `service.rs`

- Writes: `let mut tx = store.begin().await?;` (`BEGIN IMMEDIATE` on the single write connection), call repo, `tx.commit().await?`.
- Reads: `repo::get(&store.read, id)`. Never read through `store.write`.
- `now()` truncates to milliseconds so the returned row equals the stored one. Copy it.
- `None` becomes `AppError::NotFound(format!("no hello {id}"))`.
- Tests open a real store on a `tempfile::tempdir()` and return `TestResult`. No mocks.

### `http.rs`

- `pub fn routes() -> OpenApiRouter<AppState>` with `.routes(routes!(create, list))`. Handlers on the same path share one `routes!`.
- Request type `CreateHello` derives `Deserialize, ToSchema`. A required string uses `#[serde(default)]` plus `#[schema(required = true)]`, so a missing key gets the same 422 as an empty one.
- Response type `HelloView` derives `Serialize, ToSchema`, with `#[schema(as = Hello)]` for the spec name. Build it with `impl From<Hello> for HelloView`. Never serialize the model.
- Timestamps in views: `#[serde(with = "time::serde::rfc3339")]` and `#[schema(value_type = String, format = DateTime)]`.
- The handler parses (`HelloName::parse(&body.name)?`), calls the service, and maps. No business logic, no SQL.
- A path id that does not parse is `AppError::NotFound`, the same as a missing row.

### `tests/api/hello.rs`

- `spawn_app().await?` gives an in-process server on a fresh data dir. Keep the `TestApp` alive for the whole test.
- Snapshot JSON with `insta`, redacting `id`, `created_at`, `updated_at`, `instance` (see `redacted()`).

## utoipa placement

All annotations sit on the handler in `http.rs`:

```rust
/// Create a hello
///
/// Trims the name, which must then be 1 to 80 characters.
#[utoipa::path(
    post,
    path = "/api/hello",
    operation_id = "hello_create",
    tag = "hello",
    request_body = CreateHello,
    responses((status = 201, description = "The new hello", body = HelloView)),
)]
```

- `path` is the real `/api/...` path.
- `operation_id` is `<feature>_<verb>`. hey-api turns it into `helloCreateMutation`, `helloListOptions`, `helloListQueryKey`. Renaming it breaks the web imports.
- `tag` is the feature name.
- List only success responses. `http::openapi` adds the `Problem` `default` response to every operation.
- The doc comment's first line is the summary, the rest is the description.
- `params(("id" = String, Path, description = "…"))` for path parameters.

## AppError mapping

| variant | status | slug | `detail` | use for |
|---|---|---|---|---|
| `NotFound(String)` | 404 | `not-found` | the string | missing row, malformed path id |
| `Validation(Vec<FieldError>)` | 422 | `validation` | `the request is invalid`, plus `errors` | `parse` failures, one `FieldError` per bad field |
| `Conflict(String)` | 409 | `conflict` | the string | unique or state clash the caller can fix |
| `PayloadTooLarge` | 413 | `payload-too-large` | fixed | set by middleware, rarely by hand |
| `Internal(anyhow::Error)` | 500 | `internal` | none | a bug or impossible state; logs the whole chain |
| `Db(sqlx::Error)` | 500 | `internal` | none | comes from `?` on any sqlx call |

- `?` converts `sqlx::Error` and `anyhow::Error`. Map anything else with `.map_err(|err| AppError::Internal(err.into()))`, as `timestamp()` does.
- 5xx never carries `detail`. Put the facts in the error chain; the log line shows them.
- Write 4xx strings for the user: `no hello {id}`, lower case, no trailing period.
- Do not add a variant for one feature. If none fits, ask.

## `#[instrument]` rules

- Name spans `<feature>.<verb>`: `#[instrument(name = "hello.create", …)]`. Instrument service functions, not handlers or repo calls. The `http` request span already wraps the handler.
- Skip large or secret arguments. `skip_all` when the arguments are domain values; `skip(store)` when the rest are small (`hello.list` keeps `limit`).
- Record ids you create: declare `fields(hello.id = field::Empty)`, then `Span::current().record("hello.id", field::display(hello.id))`.
- Field names are `<feature>.<field>`, dotted, snake_case.
- The JSONL file drops span fields and span closes. After a write, also log a debug event with the id: `tracing::debug!(hello.id = %hello.id, "hello created")`. Messages are lower case.
- Never log secrets. Wrap them in `config::Secret<T>`.

## When you are about to…

| about to | do instead / also |
|---|---|
| add a field to an entity | follow "Add a field" below; all five files change |
| add a table | new migration, then `just db-prepare`, then `just db-schema` |
| edit an applied migration | add a new migration |
| write SQL outside `repo.rs` | move it to `repo.rs` |
| import `sqlx` in `http.rs` | call a service function |
| return `anyhow::Result` from a feature | return `crate::error::Result` |
| `unwrap()` / `expect()` / `println!` | `?`, `ok_or_else(...)`, `tracing::debug!`; clippy denies them, tests too |
| read `std::env::var` | add a `config::Config` field |
| build a path by hand | use `config::Paths` |
| serialize a model struct | add a `*View` in `http.rs` with `From<Model>` |
| change a handler, view, or `#[utoipa::path]` | `just openapi`, then `just gen-api` |
| change a `query!` | `just db-prepare` |
| change a response body | update the snapshot (see below) and review the diff |
| add a dependency | check the allowlist in `docs/ARCHITECTURE.md`; ask if absent |
| add a feature | copy `features/hello/` whole, rename, register in `features/mod.rs` |

## Add a field (worked checklist)

Say hello gains `greeting: String`, 1 to 40 characters.

1. `migrations/0002_hello_greeting.sql`: `ALTER TABLE hello ADD COLUMN greeting TEXT NOT NULL DEFAULT 'hello' CHECK (length(greeting) BETWEEN 1 AND 40);`. `NOT NULL` needs a `DEFAULT` for existing rows.
2. `model.rs`: a `HelloGreeting` newtype with `parse`, a field on `Hello` and `NewHello`, parse tests.
3. `repo.rs`: the column in `INSERT` and in every `SELECT` (`greeting AS "greeting: HelloGreeting"`).
4. `http.rs`: the field on `CreateHello` and `HelloView`, parsed in the handler, mapped in `From<Hello>`.
5. `web/src/features/hello/hello-page.tsx`: the input and its display. See the `web-conventions` skill.

Then regenerate, in this order:

```sh
just db-prepare                 # .sqlx/ for the changed queries
just db-schema                  # schema.sql
just openapi                    # openapi.json
just gen-api                    # web/src/api/
INSTA_UPDATE=always just test   # rewrite snapshots, then read the git diff
just check
```

Also update `service.rs` tests, `tests/api/hello.rs` cases, the web test fixtures in `hello-page.test.tsx`, and `docs/ARCHITECTURE.md` when it names the shape. Commit the generated files with the change. Drift fails prek and CI.
