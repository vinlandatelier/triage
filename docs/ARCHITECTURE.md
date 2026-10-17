# Architecture

Cross-initiative facts. The loop reads this file every session. Any PR that adds a crate, module, invariant, or dependency updates it in the same PR.

## Shape

One binary, `triage`, is daemon and CLI. A React SPA is served by the daemon in release and by Vite in dev. Discord is the phone surface. Claude Code runs as a subprocess with the user's own login; there are no API keys.

The tree is the target layout. Files appear as the tickets that need them land.

```
triage/
  Cargo.toml  rust-toolchain.toml  rustfmt.toml  deny.toml  bacon.toml  justfile
  package.json  bun.lock   bun workspace, members web/ and tools/gen-api/
  .bun-version       bun version for CI and `just setup`
  prek.toml  renovate.json  .github/workflows/
  .cargo/config.toml   SQLX_OFFLINE=true
  scripts/           check-layout.sh claude-format.sh db-schema.sh
  tools/gen-api/     openapi-ts.config.ts nocheck.ts, hey-api pinned to TypeScript 6
  crates/triage/
    Cargo.toml  build.rs  migrations/  .sqlx/  openapi.json  schema.sql
    src/
      main.rs            parse → cli::run (config → telemetry → command) → exit code
      lib.rs
      cli.rs             subcommands
      config.rs          env → Config, Paths
      error.rs           AppError → problem+json
      telemetry.rs       tracing layers, dormant otel
      telemetry/         columns.rs (stderr formatter) otel.rs (OTLP exporter)
      http.rs            serve: lock, bind, graceful shutdown
      http/              router.rs state.rs middleware.rs assets.rs openapi.rs probe.rs
      store.rs           Store, store::Error
      store/             pool.rs query.rs schema.rs
      guardrails/        placeholder
      features/
        mod.rs           registers feature routers
        <name>/          mod.rs http.rs service.rs repo.rs model.rs
    src/bin/gen_openapi.rs
    tests/api/           main.rs helpers.rs <feature>.rs snapshots/
  web/
    package.json vite.config.ts biome.json tsconfig.json env.ts index.html prek.toml components.json
    playwright.config.ts
    e2e/               Playwright specs against the release binary
    doctor.config.ts knip.json
    src/
      main.tsx router.ts routeTree.gen.ts error.ts fallbacks.tsx index.css api-config.ts query.ts
      lib/utils.ts       cn()
      components/        theme-provider.tsx theme-toggle.tsx
      routes/            file-based
      api/               hey-api output, never edited
      components/ui/     shadcn vendored
      features/<name>/
      test/
  .claude/
    settings.json        PostToolUse edit hook
    skills/              committed skills
```

## Rules

A rule that names a check (CI, `just`, prek) holds now; the check enforces it once the ticket that adds the check lands.

1. One crate until a trigger fires: a heavy independent dependency set, a second binary, or incremental `cargo check` over 10 s. Then split the module into a new crate under `crates/<name>`.
2. Product code lives in `features/<name>/` with exactly `mod.rs http.rs service.rs repo.rs model.rs`. Infra stays top level. `just check-layout` checks the shape.
3. Direction is `http → service → repo`. `http.rs` never imports `sqlx`; `repo.rs` never imports `axum`. `just check-layout` greps for it.
4. All fallible code returns `crate::error::Result<T>`. `AppError` is the only error type that crosses a feature boundary. Startup code (`config`, `http::serve`, `cli`) keeps its own error types, which map to exit codes. `anyhow` only in `main.rs`, `cli.rs`, `build.rs`, and as the payload built for `AppError::Internal` (`anyhow!`, `.context()`).
5. `unwrap`, `expect`, `dbg!`, `todo!`, `println!`, `eprintln!` are clippy-denied. `clippy.toml` allows all but `todo!` in tests. `?` and `tracing` instead. The one exception: `cli.rs` prints command output to stdout under `#[expect(clippy::print_stdout, reason = "CLI output")]`.
6. All SQL lives in `repo.rs`, through `sqlx` macros, offline data committed in `.sqlx/`. Migrations are forward-only `.sql` files applied at startup.
7. Every path comes from `config::Paths`. No `~` or hardcoded dirs anywhere else.
8. `std::env::var` appears only in `config.rs`, and `println!` only in `cli.rs`. `just check-layout` greps `crates/triage/src/` for both.
9. `triage serve` runs in the foreground, logs to stderr, exits cleanly on SIGTERM and SIGINT, holds a lock on the data dir. It never forks.
10. The frontend never hand-writes a request type. Everything comes from `web/src/api/`, generated from `openapi.json`.
11. Server state lives in TanStack Query. View state lives in the URL via TanStack Router. No global client store until a feature proves it needs one.
12. Every check runs through a `just` recipe. CI calls the same recipes.
13. No new dependency outside the allowlist below without asking.
14. Any change that adds, removes, or re-versions a dev tool (a CLI, a cargo subcommand, a prek hook needing a binary, a runtime) updates `just setup` in the same PR. A fresh clone plus `just setup` must always reach a green `just check`.

## Toolchain and lints

- `rust-toolchain.toml` pins the exact Rust version. `rust-version` in the root `Cargo.toml` matches it.
- Lints live in `[workspace.lints]`. Every crate sets `lints.workspace = true`.
- Lint group entries (`all`, `pedantic`) carry `priority = -1` so single-lint overrides win.

## Dev tools

`just setup` installs or reports each one: rustup and the pinned toolchain, cargo-binstall, cargo-nextest, cargo-deny, cargo-machete, sqlx-cli, bacon, prek, bun, Node 24, actionlint. Cargo tools come from `cargo binstall`, falling back to `cargo install`. macOS uses Homebrew for the rest; Linux uses official installers or release binaries. `.bun-version` pins bun: `just setup` installs that version when bun is missing and warns when the installed bun differs, and CI reads it through `oven-sh/setup-bun`. It ends with `bun install` at the root, `bunx playwright install chromium` from `web/` (the root has no `node_modules/.bin`, so `bunx` there would fetch the latest Playwright instead of the pinned one), and `prek install`, which installs the `pre-commit` and `commit-msg` hooks.

- `just` ≥ 1.42, enforced at the top of the `justfile`.
- A `just` recipe longer than about 5 lines moves to `scripts/<recipe>.sh`; the recipe calls it. `scripts/setup.sh` and `scripts/skills-update.sh` follow this, and `just setup` passes the caller's own `PATH` as `TRIAGE_USER_PATH` so the script can warn about tools a plain shell would not find.
- `bacon.toml` holds the watch jobs. `just dev` serves the app at `localhost:$TRIAGE_DEV_PORT` (default 7311). It first fails with "installed triage is running on 7311, stop it or set TRIAGE_DEV_PORT" when something answers on `127.0.0.1` at that port, because Vite may bind `::1` and miss it. Then it runs, in parallel, `bacon --headless run-server` with `TRIAGE_PORT=7312` and `bun run dev --port $TRIAGE_DEV_PORT`. `web/vite.config.ts` sets `strictPort` and proxies `/api` to `127.0.0.1:7312`; the port is fixed there because the config cannot read env without `@types/node`.
- `prek.toml` holds the commit hooks. commitlint and its conventional config live in a prek-managed node env, not in `package.json`. actionlint, cargo, and `just` come from the system.
- `renovate.json` opens grouped non-major updates weekly, plus lock file maintenance. Regex custom managers bump the pins in `prek.toml`. Automerge is off.
- `deny.toml` allows MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, MPL-2.0, Zlib, and crates.io as the only source.

## Dependency allowlist

Rust: tokio, axum, tower, tower-http, hyper, serde, serde_json, sqlx, utoipa, utoipa-axum, utoipa-scalar, tracing, tracing-subscriber, tracing-appender, tracing-opentelemetry, opentelemetry, opentelemetry_sdk, opentelemetry-otlp, figment, dotenvy, thiserror, anyhow, problem_details, clap, clap_complete, directories, rust-embed, mime_guess, ulid, time, uuid, url. Dev: axum-test, insta, tempfile, pretty_assertions, tokio-test.

Web: react, react-dom, @tanstack/react-query, @tanstack/react-router, @tanstack/router-plugin, @hey-api/openapi-ts, zod, tailwindcss, shadcn and its vendored deps (`@base-ui/react`, `tw-animate-css`), class-variance-authority, clsx, tailwind-merge, lucide-react. Dev: vite, @vitejs/plugin-react, @tailwindcss/vite, typescript, @biomejs/biome, vitest, jsdom, @testing-library/react, @testing-library/dom, @testing-library/user-event, msw, @playwright/test, @julr/vite-plugin-validate-env, knip, react-doctor, ts-reset (`@total-typescript/ts-reset`), @types/react, @types/react-dom.

## Identifiers

Binary, crate, workspace: `triage`. Env prefix: `TRIAGE_`. `ProjectDirs::from("studio", "", "triage")`. Web package: `@triage/web`.

## Paths

| | macOS | Linux |
|---|---|---|
| data, db | `~/Library/Application Support/studio.triage/` | `$XDG_DATA_HOME/triage/` |
| logs | `~/Library/Logs/triage/` | `$XDG_STATE_HOME/triage/logs/` |
| config | `~/Library/Application Support/studio.triage/` | `$XDG_CONFIG_HOME/triage/` |

On Linux, an unset XDG var falls back to `~/.local/share`, `~/.local/state`, or `~/.config`.

`--data-dir <path>` or `TRIAGE_DATA_DIR` puts everything under one directory: data at its root, then `logs/` and `config/`. The flag wins over the env var and logs a debug line when it does. Tests always use it. A relative data dir is made absolute and a leading `~` exits 2. `Paths::resolve` creates missing dirs with mode 0700 and exits 1 when the data or config dir is not writable or there is no absolute `HOME`. An unusable logs dir does not fail resolution; telemetry reports it (see Observability).

## Config

`config::Config` is extracted by figment from `TRIAGE_*` env vars, with `.env` read in debug builds only. `config::REQUIRED` lists vars that must be set; the preflight names every missing one in one error. A missing or malformed var exits 2 naming the var. Wrap secret values in `config::Secret<T>`, whose `Debug` prints `[redacted]`. Startup errors stay outside `AppError`: config code returns `config::Result`. `otel_endpoint` falls back to the standard `OTEL_EXPORTER_OTLP_ENDPOINT` when `TRIAGE_OTEL_ENDPOINT` is unset.

## CLI

`cli.rs` owns the clap definition and dispatch; `main.rs` only parses and maps errors to exit codes. `triage config show` prints every `Config` var and each resolved path, aligned, with values formatted through `Debug` so `Secret<T>` prints `[redacted]`. URL userinfo prints as `redacted`. `Config::entries` destructures `Config` exhaustively, so a new field must be added there to compile. `triage completions <bash|zsh|fish>` writes a script to stdout and runs before config loads. `triage logs path` prints the absolute path of today's JSONL file (UTC date), whether or not it exists yet; `telemetry::log_file` builds the name from the same prefix and suffix as the appender. `triage status` sends `GET /health` to `127.0.0.1:$TRIAGE_PORT` through `http::probe`, a hand-written HTTP/1.1 request over `tokio::net::TcpStream` with a 2 s timeout and a 16 KiB response cap, because no HTTP client crate is allowlisted. It prints `running vX.Y.Z on 127.0.0.1:<port>, up 3h12m` and exits 0, or prints `not running` and exits 1 when nothing answers or the answer is not a triage health body. `cli::run` returns the process `ExitCode`. `triage db schema` opens and migrates the store on the data dir, then prints the schema through `store::dump_schema`. `triage db migrate` applies pending migrations and prints `applied N migration(s)`. `triage db check` and `triage db sql` open the file through `store::open_read_only`, one `SQLITE_OPEN_READONLY` connection that never creates the database file or migrates it, so writes fail inside SQLite with `readonly database`. A missing file exits 1 and names `triage db migrate`. `check` compares the live dump to `store::SCHEMA`, the `schema.sql` embedded at build time, and on drift prints the first differing line and exits 1. `sql "<query>"` runs exactly one statement through `store::query`; it rejects empty or multi-statement input before running anything, because sqlx would run every statement under the first one's column names. `store::query` decodes each cell by its storage class (blobs as `x'…'`, invalid UTF-8 replaced, non-finite reals as strings). The default output is an aligned table with `NULL`, and cells and names escaped like the `columns` log format (`⏎`, `\xNN`). `--json` prints an array of row objects with sorted keys and rejects duplicate column names.

## Server

`triage serve` (`http::serve`) takes `Paths::lock_file` (`<data>/triage.lock`) with std `File::try_lock` before it binds; no pid is written and the OS drops the lock when the process dies. A held lock exits 1 with "another triage instance is using <data dir>". It binds `127.0.0.1:$TRIAGE_PORT` only; a port in use exits 1 naming the port and `TRIAGE_PORT`. On macOS, tokio sets `SO_REUSEADDR`, so the bind succeeds while another process listens on `0.0.0.0` at the same port, and triage takes the loopback traffic instead of failing; this gap is accepted. Signal handlers are registered before the bind, so a signal during startup still drains. SIGINT or SIGTERM stops accepting, drains open requests for up to 10 s, then exits 0. A second SIGINT or SIGTERM during the drain logs a warning and exits 0 at once. After a drain where every request finished, `Store::close` closes both pools, so the last connection checkpoints the WAL and removes the `-wal` and `-shm` files. After the lock and before binding, it opens the store (see Storage). Startup logs `listening` with version, address, data dir, db file, and log file; exit logs `stopped`, also when the server fails after binding. `AppState` holds `Arc<Config>`, `Arc<Paths>`, the `Store`, and the start `Instant`. `GET /health` returns `{"status":"ok","version","uptime_s"}`. `http::middleware::trace` wraps the router: it sets `x-request-id` to a UUIDv7, opens the request span, and copies the id onto the response. An incoming id is kept when it is 1 to 128 URI-unreserved characters (letters, digits, `-._~`) starting with a letter or digit; otherwise it is replaced. The per-request event `response` (`status`, `latency_ms`) logs at debug, so under the default `TRIAGE_LOG_FILE_LEVEL` the JSONL file gets one line per request and stderr shows only the span's close line. A 5xx raises `response` to warn; a body or stream error logs `response failed` at error. `http::assets::fallback` is the router fallback and serves the web UI from `web/dist/` through `rust-embed` (with `mime-guess`). Release builds embed the files and fail to compile without `web/dist/`; debug builds read it from disk with `allow_missing`, so `cargo` works before any web build. `/assets/*` files, hashed by Vite, get `Cache-Control: public, max-age=31536000, immutable`; `index.html` and other files get `no-cache`. A missing path with no extension in its last segment returns `index.html` with 200 so client routes load; a missing `/assets/*` path or other file returns a `NotFound` problem, as does every unknown `/api` path. `just build` runs `just build-web`, then `cargo build --release --locked`. Startup errors stay outside `AppError`: `http::serve` returns `http::Result` and `cli::Error` wraps `config::Error` and `http::Error`, each with its own exit code; `http::Error::Store` carries `store::Error`.

## Storage

`store::open(&Paths)` opens `Paths::db_file` (`<data>/triage.db`), creating it if missing, and returns `Store { read, write }`, two `SqlitePool`s on the same file. Every connection sets WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`, and `temp_store=MEMORY`. The read pool holds up to `available_parallelism` connections with `query_only=ON`. The write pool holds one connection, so writers queue in the pool instead of contending for the file lock. Write through `Store::begin`, which issues `BEGIN IMMEDIATE`. `sqlx::migrate!()` embeds `crates/triage/migrations/` and runs on the write pool at open; `build.rs` reruns when that dir changes. sqlx runs each migration file in its own transaction, so a failure leaves earlier files applied. `crates/triage/schema.sql` holds the migrated schema: `SELECT sql FROM sqlite_schema WHERE sql NOT NULL AND name NOT GLOB 'sqlite_stat*' ORDER BY type DESC, name` (`ANALYZE` creates the `sqlite_stat*` tables at runtime), each statement ending in `;`, because `.schema` order is not stable. Read it instead of replaying migrations. `just db-schema` (`scripts/db-schema.sh`) builds the binary and runs `triage db schema` on a fresh temp data dir, from that dir with only `HOME` and `PATH` set, so no `.env` or caller `TRIAGE_*` or `OTEL_*` var reaches it. `schema::schema_dump_is_current` fails with a diff when `schema.sql` is stale, `schema::db_schema_prints_schema_sql` fails when the command's output differs from it, and prek runs that test through `just db-schema-check` when `migrations/` or `schema.sql` changes. Startup exits 1 naming the database path when the file cannot be opened or is not a database, naming the migration file when one fails, and naming the file and `checksum` when an applied migration was edited. A database with a migration this binary does not know (`store::Error::Newer`) exits 1 asking to upgrade triage.

Queries go through `sqlx::query!`/`query_as!` in `repo.rs`, compiled against `crates/triage/.sqlx/`, which is committed so builds need no `DATABASE_URL`. `.cargo/config.toml` sets `SQLX_OFFLINE=true`, so the macros read `.sqlx/` even when `DATABASE_URL` is set and a stale entry fails the build; `cargo sqlx prepare` overrides it. `just db-prepare` regenerates it and `just db-prepare-check` (in `just check` and the CI `sqlx` job) fails when it is stale. Both migrate a temp database with sqlx-cli, not `triage db migrate`, because the crate cannot build while `.sqlx/` is stale. After adding a migration, run `just db-prepare` before `just db-schema`. Ids are ULID text through a per-feature newtype that implements `sqlx::Type` (`hello::model::HelloId`); select them with `id AS "id: HelloId"`. Timestamps are `time::OffsetDateTime` in UTC, written by `repo.rs` as fixed-width `YYYY-MM-DDTHH:MM:SS.mmmZ` so text order is time order (sqlx's own RFC 3339 encoding drops trailing subsecond zeros), and read back through sqlx's `time` feature. Services truncate `now` to milliseconds so a returned row equals the stored one.

## Features

`features::routes()` merges every feature's `http::routes()` into `http::router::routes`. `features/hello/` is the worked example: `model.rs` holds validated newtypes (`HelloName::parse` trims, then requires 1 to 80 characters, else `AppError::Validation` on `name`); `repo.rs` holds the SQL; `service.rs` writes inside `Store::begin` and reads from `store.read`, with `#[instrument(name = "<feature>.<verb>")]` spans that record the new id (`hello.id`). `http.rs` parses the request, calls the service, and maps the result: `POST /api/hello` (201), `GET /api/hello` (20 newest), `GET /api/hello/{id}` (a malformed id is a 404, like a missing one). Feature routes use their real `/api/...` path in `#[utoipa::path]`. Response types are `ToSchema` view structs in `http.rs` (`HelloView`, named `Hello` in the spec) built from the model, with timestamps as RFC 3339 through `time::serde::rfc3339`. The JSONL file logs no span closes and keeps span fields off its lines, so `service::create` also logs a debug `hello created` event carrying `hello.id`.

## Errors

Handlers return `crate::error::Result<T>` with `AppError`: `NotFound(String)` 404, `Validation(Vec<FieldError>)` 422, `Conflict(String)` 409, `PayloadTooLarge` 413, `Internal(anyhow::Error)` 500, `Db(sqlx::Error)` 500 (slug `internal`, logs `database error`). Each renders as RFC 9457 `application/problem+json` through `problem_details`: `type` is `https://triage.studio/problems/<slug>`, `title`, `status`, `detail` (4xx only, so 5xx never leak internals), `errors` for validation. `Internal` logs `internal error` at error level with `error` recorded as `&dyn Error`, so both sinks show the whole chain. `http::middleware::problems` sits inside `trace`: it caps extractor bodies (`Bytes`, `String`, `Json`, `Form`) at 1 MiB (`error::BODY_LIMIT`), turns a handler panic into an `Internal` 500 with tower-http `CatchPanicLayer`, turns any plain-text 413 (axum's body-limit rejection) into a problem, and sets `instance` to the request id. It swaps only the body, so headers the handler set survive. `AppError::into_response` stores the `Problem` in the response extensions so that middleware can add `instance`. Unknown `/api` paths fall back to a `NotFound` problem; every other unknown path goes to `http::assets` (see Server).

## Contract

Routes register through `utoipa_axum::router::OpenApiRouter` in `http::router::routes`, each handler annotated with `#[utoipa::path]` and an explicit `operation_id` of the form `<feature>_<verb>` (`health_get`), so hey-api names read `healthGetOptions`. `http::api_doc()` splits that router and returns the spec: OpenAPI 3.1, title `Triage`, version from Cargo, server `http://127.0.0.1:7311`. It registers `Problem` and `FieldError` as schemas and a `Problem` response (`application/problem+json`), and gives every operation that response as its `default`. `problem_details` implements only utoipa 5's `ToSchema`, so `error::ProblemSchema` describes the body instead and must change when `Problem` does. The daemon serves the spec at `GET /api/openapi.json`, outside the spec. Debug builds also serve Scalar at `GET /api/scalar` (`utoipa-scalar`, `#[cfg(debug_assertions)]` in `http::router`), with the spec inlined; release builds leave that path to the `/api` fallback, a `NotFound` problem. `crates/triage/openapi.json` is the committed copy, written by `src/bin/gen_openapi.rs` through `http::api_json` (keys sorted via `serde_json::Value`, trailing newline); the spec is a runtime value, so a bin builds it, not `build.rs`. `just openapi` regenerates it; `openapi::openapi_spec_is_current` fails with a diff when it is stale, and prek runs that test through `just openapi-check` when anything under `crates/triage/src/`, `openapi.json`, either `Cargo.toml`, or `Cargo.lock` changes. `openapi::problem_schema_matches_the_body` fails when `ProblemSchema` and a rendered problem disagree on keys. The package sets `default-run = "triage"` so `cargo run` still means the daemon.

## Web

`web/` is a Vite SPA. Routing is TanStack Router with file-based routes in `web/src/routes/`. `@tanstack/router-plugin/vite` runs with `autoCodeSplitting` and sits before `react()` in `vite.config.ts`, after `ValidateEnv()`. It writes `web/src/routeTree.gen.ts` on every `vite dev` and `vite build`; the file is committed, never edited, and excluded from Biome. knip needs no ignore for it, since nothing in it goes unused. `router.ts::createAppRouter` builds the router and registers its type; `main.tsx` and the tests both call it. `routes/__root.tsx` is the shell: a header and `<Outlet/>`. `fallbacks.tsx` holds `NotFound` and `RouteError`; the root route uses them and `createAppRouter` sets them as `defaultNotFoundComponent` and `defaultErrorComponent`, so a child route's error or `notFound()` renders inside `<Outlet/>` and keeps the header. `RouteError` shows the message and the request id when there is one, and its retry calls `reset()` plus `router.invalidate()` so a failed loader runs again. `web/src/error.ts::describeError` reads a problem+json body (`detail`, then `title`, request id from `instance`) or an `Error` (`message`, `requestId`); empty strings count as missing.

`web/src/api/` is the hey-api 0.99 output for `crates/triage/openapi.json`: types, an SDK (`healthGet`), and TanStack Query options (`healthGetOptions`). It is committed, never edited, and excluded from Biome, knip, react-doctor, and coverage. `just gen-api` (`bun run gen:api`) regenerates it. The generator lives in its own workspace, `tools/gen-api/`, because hey-api needs the TypeScript compiler API, which TypeScript 7 does not ship; that workspace pins TypeScript 6 and nothing in `web/` imports it. hey-api copies its fetch runtime into `src/api/client/` and `src/api/core/`, and `nocheck.ts` marks those files `// @ts-nocheck` because they fail `exactOptionalPropertyTypes`; the schema-derived files stay checked. `src/api-config.ts` sets the client base URL to `VITE_API_URL` (default `/`, because spec paths already start with `/api`), resolved against `location.origin` because Node's fetch rejects a relative URL in tests. The client runs with `throwOnError`. `src/query.ts` registers `toProblemError` as the client's error interceptor, so every failed response rejects with `ProblemError` (`status`, `title`, `detail`, `instance`, `errors`); a body that is not a problem keeps the response status. A fetch that gets no response rejects with a `TypeError`, which becomes `NetworkError` ("Cannot reach the triage daemon"); an abort keeps its own error. The interceptor is registered there because the generated client imports `api-config.ts` while it loads. `createQueryClient()` sets `retry: 1`, `staleTime: 30_000`, and `throwOnError` for `ProblemError` with status 500 or more, so 5xx reaches the route `errorComponent` and 4xx stays with the component. The router context carries `queryClient` (`createRootRouteWithContext`), with `defaultPreloadStaleTime: 0` so Query decides freshness. `main.tsx` reloads once on `vite:preloadError`, when a deploy removed a chunk an open tab still points at; a `sessionStorage` timestamp stops a reload loop within 10 s.

Styling is Tailwind 4 through `@tailwindcss/vite` (last in `plugins`) and shadcn v4 with Base UI (`components.json`, style `base-nova`). `src/index.css` holds the Tailwind imports and the shadcn theme variables; the `dark` variant matches descendants of `.dark` on `<html>`, and `color-scheme` follows the theme so native controls match. Add a component with `bunx shadcn add <name>`, which runs the pinned CLI, from `web/`; it lands in `src/components/ui/` and is then ours to edit. Imports use `@/` for `src/`, defined once in `tsconfig.json` `paths` and read by Vite through `resolve.tsconfigPaths`. Biome parses Tailwind directives (`css.parser.tailwindDirectives`). shadcn's generated `utils.ts` and font import were replaced with `clsx` + `tailwind-merge` and the system font, because the code may import only allowlisted packages; re-check `package.json` after any `shadcn` command.

Feature UI lives in `web/src/features/<name>/`; the route file in `src/routes/` only wires a loader to the feature component. `features/hello/hello-page.tsx` is the worked example on `/`: the route loader calls `ensureQueryData(helloListOptions())` and the list reads it with `useSuspenseQuery`, so any list failure reaches the route `errorComponent`. The form checks the name with `name.ts::parseName`, which mirrors `HelloName::parse`, and shows "name must be 1 to 80 characters" without a request. It sends `helloCreateMutation()` and invalidates `helloListQueryKey()` on success. A 4xx from the mutation shows inline, the `name` field error first; a `NetworkError` or 5xx goes to the `errorComponent`, through the mutation's `throwOnError`.

Theme is `system | light | dark`, default `system`, stored in `localStorage` under `triage-theme`. An inline script in `index.html` sets `.light` or `.dark` before first paint; `components/theme-provider.tsx` (shadcn's Vite provider, with storage access in try/catch, a `matchMedia` listener so `system` follows the OS live, no `storageKey`/`defaultTheme` props so it cannot drift from the script, and a `useTheme` that throws outside the provider) takes over after mount. The key lives in both files. `components/theme-toggle.tsx` cycles the three values from the header.

## Observability

`tracing` is the instrumentation. Three sinks: stderr columnar for humans, JSONL file for agents, OTLP when `Config.otel_endpoint` is set. `telemetry::init(&Config, &Paths)` installs the subscriber in `cli::run` after `Paths::resolve`, so anything logged during resolution is lost; `config::log_data_dir_override` re-emits the override line afterwards. `cli::run` holds the returned guard until exit; dropping it stops file logging. Completions skip telemetry.

- stderr: filter `TRIAGE_LOG_LEVEL`, format `TRIAGE_LOG_FORMAT` (`columns`, `pretty`, `compact`). ANSI colour only when stderr is a TTY.
- `columns` (default) prints `HH:MM:SS.mmm LEVEL module  message  key=value…  duration  req=<id>`. Time is local, with the offset read per event so DST changes apply; it falls back to UTC. Module is the last segment of the target. Newlines in values print as `⏎` and other control characters as `\xNN`. Each `source()` of an event's error field adds an indented `└ ` line; the field must be recorded as `error = &err as &dyn Error`, since `%err` and `?err` lose the chain. A closing span prints its name, its fields, and its wall-clock lifetime since creation; span error fields get no `└ ` lines. `req=` comes from the event's own `request_id` or the nearest span that has one. A span below `TRIAGE_LOG_LEVEL` is invisible to this sink, so its `request_id` never reaches `req=`.
- JSONL: `<logs>/triage.<YYYY-MM-DD>.jsonl`, daily rotation, 7 files kept, lossless non-blocking writer, filter `TRIAGE_LOG_FILE_LEVEL`. Each line leads with `ts level target message`, then the event fields flattened, then `span` (innermost name), `request_id` (the event's own, else the nearest span that has one), `span_id`, `trace_id` (root span id). Ids are strings and unique only among live spans of one process. `ts` is UTC with 6 sub-second digits. `message` is always a string. An error field holds its `source()` chain joined by `: `. An event field named like a reserved key is written as `field.<name>`, prefixed again until the key is free. The file filter also gates spans, so a span below `TRIAGE_LOG_FILE_LEVEL` gives its events no span keys.
- OTLP: every opentelemetry crate is used only in `telemetry/otel.rs`, so version bumps touch one file. With no endpoint, no OTLP code runs. Otherwise it adds a `tracing-opentelemetry` layer (file filter) with a batch exporter over HTTP/protobuf to `<endpoint>/v1/traces`, resource `service.name=triage` and `service.version`. Building never connects. Logged endpoints have their userinfo redacted. The first failed export logs one warning; later failures are silent. Dropping the guard shuts the provider down, which flushes pending spans before the file writer stops; each export, retries included, gets 2 s and the shutdown 5 s. The exporter still reads the other `OTEL_EXPORTER_OTLP_*` vars itself (headers, compression).
- An unwritable logs dir or an invalid filter logs one warning and continues on stderr or the default filter.

Every HTTP request opens a span `http` with `method`, `path` (no query), `request_id`, and `status`, recorded on response. Events inside a handler inherit `request_id`. A future `agent_run` span carries `session_id`, `model`, `cost_usd`, `num_turns`.

## Testing

Integration tests live in one binary, `crates/triage/tests/api/`: `main.rs` declares the modules, `helpers.rs` holds the harness, and each feature gets its own file. `helpers::spawn_app()` returns `TestApp { server, store, dir }`: a fresh `TempDir` as the data dir, a migrated `Store` on it, and an in-process `axum_test::TestServer` over `http::router`. Dropping `dir` deletes the database, so keep the `TestApp` alive for the whole test. nextest runs each test in its own process. Tests that need the real binary (signals, stderr, `triage status`) spawn `CARGO_BIN_EXE_triage` with `--data-dir` instead. Feature tests snapshot JSON bodies with `insta` (`json`, `redactions`), redacting `id`, `created_at`, `updated_at`, and `instance`; snapshots live in `tests/api/snapshots/`. `schema::schema_conventions` checks every ordinary table in `main` except `_sqlx_migrations` and SQLite's internal `sqlite_*` tables; virtual tables and their shadow tables are skipped. It fails when one is not `STRICT`, lacks `id TEXT PRIMARY KEY`, `created_at`, or `updated_at`, or has a name that is not snake_case. axum-test turns on axum's `tracing` feature in test builds, so log-capture tests filter to the `triage` target.

Web tests run on Vitest with jsdom, configured in the `test` block of `web/vite.config.ts`, so they share the app's plugins and `@/` alias. `bun run test` runs them once; `just check-web` calls it. Vitest picks up only `src/**/*.test.ts(x)`; keep those files out of `src/routes/`, where the router plugin would treat them as routes. `web/src/test/setup.ts` starts one MSW server (`msw/node`). A request without a handler rejects, and `afterEach` fails the test that made it even when the caller caught the rejection, as TanStack Query does. Add handlers per test with `server.use(...)` from `@/test/setup`; they reset after each test, as do rendered trees, the theme class on `<html>`, and `localStorage`. Build API handlers with `mock<Res>(method, path, resolver)` from `@/test/handlers`: `path` is the spec path (`/health`) under the client base URL, and `Res` is the generated response type (`HealthGetResponse`), so a resolver that returns `HttpResponse.json` of another shape, other than a `Problem`, fails `tsc`. A plain `Response` skips the check, for non-JSON bodies. `openapi-msw` is not used: it needs an openapi-typescript `paths` type that hey-api does not emit. The setup stubs `window.matchMedia`, which jsdom lacks, and `window.scrollTo`, which jsdom only logs as not implemented. `src/test/shell.test.tsx` renders the app router from `createAppRouter` on a memory history; copy its `renderAt` for route-level tests. `features/hello/hello-page.test.tsx` copies it with `retry: false`, because the default retry waits longer than `findBy`. Drive forms with `@testing-library/user-event`.

## Quality gates

| layer | trigger | runs |
|---|---|---|
| edit hook | Claude Code PostToolUse on Edit/Write | `rustfmt` or `biome check --write` on that file |
| prek | commit | fmt, clippy, layout check, biome, react-doctor, schema, openapi, and api client drift, builtin file checks, actionlint, commitlint |
| `just check` | agent before PR, human anytime | fmt, clippy, layout, biome, react-doctor, schema, openapi + tsc, nextest, vitest, knip, cargo-deny, cargo-machete, sqlx offline check, api client drift |
| CI | PR | `just check` split into jobs, ubuntu + macos for nextest, plus `just build-web`, then `just e2e` |

`scripts/check-layout.sh` enforces rules 2, 3, and 8. It prints `file: rule` per violation and exits 1 on any. It ignores `//` comments, so docs may name the forbidden items.

`.claude/settings.json` runs `scripts/claude-format.sh` after every Edit or Write. The script reads `tool_input.file_path` from the hook JSON with `sed`, since `jq` is not a dev tool. A `.rs` file goes through `rustfmt` alone, because `cargo fmt` reformats the whole workspace. A `.ts`, `.tsx`, `.js`, `.jsx`, `.json`, or `.css` file under `web/` goes through `web/node_modules/.bin/biome check --write`, run from `web/`; `bunx` is avoided because it downloads biome when it is missing. The script prints nothing and always exits 0. A missing formatter prints one `systemMessage` per tool per session, tracked by a marker file in `$TMPDIR`.

`just check-web` runs `bun run lint`, `typecheck`, `doctor`, `knip`, and `test`, in that order. `doctor` is react-doctor with `-y --no-telemetry --blocking warning`, so a warning fails it. `web/doctor.config.ts` keeps the recommended rules and turns off the Socket.dev supply-chain scan, which makes one network request per dependency. react-doctor turns on its `tanstack-query` rules by itself once `@tanstack/react-query` is a dependency. `web/knip.json` adds `src/routes/**/*.tsx`, `env.ts`, and `doctor.config.ts` as entries; everything else is knip's zero-config defaults. `src/api/**` is in knip's `ignore`.

The CI `web` job runs `bun install --frozen-lockfile`, `just check-web`, `just gen-api-check`, then `just build-web`, and uploads `web/dist/` as the `web-dist` artifact, kept 7 days. The `test` job needs `web` and downloads that artifact into `web/dist/` before `just test`.

`just e2e` runs `just build`, then Playwright (`bun run e2e`) on Chromium only. Playwright needs Node 24, not bun. `web/playwright.config.ts` starts `target/release/triage serve` as its `webServer` on port 7313, off 7311 and 7312 so a running daemon does not collide, with `--data-dir` set to a fresh `mktemp -d` so every run starts with an empty database. It waits on `/health` and never reuses a running server. Specs live in `web/e2e/*.spec.ts`; Vitest's `include` is limited to `src/**/*.test.{ts,tsx}` so it skips them. `just check` does not run e2e. The CI `e2e` job needs `test`, installs Node 24 and Chromium with its system deps, runs `just e2e`, and uploads `web/test-results/` (traces kept on failure) when it fails.

`just gen-api-check` regenerates `web/src/api/` and fails when the result differs from the git index or adds untracked files, so staged output passes. prek runs it when `openapi.json`, `tools/gen-api/`, or `web/src/api/` changes.

prek runs `web/prek.toml` as a workspace project, found by discovery, with paths relative to `web/`. Its hooks run `biome check --write` on staged web files and `react-doctor --staged` when a staged file is `.tsx`. `tsc` is a `manual` stage hook (`prek run --stage manual tsc`) because it checks the whole project. After adding or moving a nested `prek.toml`, run `prek list --refresh`; prek caches the workspace layout.

## Agent skills

`.claude/skills/` holds third-party skills, one directory each, vendored with `npx skills add` and pinned in `.claude/skills/SOURCES.md` (repo, path, commit, local edits). Local edits adapt them to these rules: `rust-best-practices` uses `AppError` and drops facet and aliri_braid, `rust-skills` keeps only the `err- type- api- async- proj- name- test- obs- serde- lint- pat- anti-` rules, and the two Vercel skills skip server-rendering rules because the web app is a Vite SPA. Where a skill disagrees with this file, this file wins. `just skills-update` applies each upstream diff since the pinned commit onto the vendored copy and bumps the commit; it never commits, and a patch that hits a local edit leaves `.rej` files to resolve by hand. Every skill directory must have a row in `SOURCES.md`.

Two skills are written here, with `local` rows that `just skills-update` skips. `rust-service-conventions` walks `features/hello/` file by file, with the `AppError` table, span rules, utoipa placement, and the regenerate commands for each change. `web-conventions` covers `src/api/`, the regenerate flow, the generated query factories, URL state, and the type rules. When this file changes a rule they show, update them in the same PR.
