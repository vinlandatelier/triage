# Architecture

Cross-initiative facts. The loop reads this file every session. Any PR that adds a crate, module, invariant, or dependency updates it in the same PR.

## Shape

One binary, `triage`, is daemon and CLI. A React SPA is served by the daemon in release and by Vite in dev. Discord is the phone surface. Claude Code runs as a subprocess with the user's own login; there are no API keys.

The tree is the target layout. Files appear as the tickets that need them land.

```
triage/
  Cargo.toml  rust-toolchain.toml  rustfmt.toml  deny.toml  bacon.toml  justfile
  package.json  bun.lock   bun workspace, member web/
  prek.toml  renovate.json  .github/workflows/
  crates/triage/
    Cargo.toml  build.rs  migrations/  .sqlx/  openapi.json  schema.sql
    src/
      main.rs            clap → cli::run (config → telemetry → command)
      lib.rs
      cli.rs             subcommands
      config.rs          env → Config, Paths
      error.rs           AppError → problem+json
      telemetry.rs       tracing layers, dormant otel
      telemetry/         columns.rs (stderr formatter) otel.rs (OTLP exporter)
      http.rs            serve: lock, bind, graceful shutdown
      http/              router.rs state.rs middleware.rs assets.rs
      store/             pool.rs
      guardrails/        placeholder
      features/
        mod.rs           registers feature routers
        <name>/          mod.rs http.rs service.rs repo.rs model.rs
    src/bin/gen_openapi.rs
    tests/api/           main.rs helpers.rs <feature>.rs
  web/
    package.json vite.config.ts biome.json tsconfig.json env.ts index.html prek.toml
    src/
      main.tsx routeTree.gen.ts
      routes/            file-based
      api/               hey-api output, never edited
      components/ui/     shadcn vendored
      features/<name>/
      test/
  .claude/
    settings.json        hooks
    skills/              committed skills
```

## Rules

A rule that names a check (CI, `just`, prek) holds now; the check enforces it once the ticket that adds the check lands.

1. One crate until a trigger fires: a heavy independent dependency set, a second binary, or incremental `cargo check` over 10 s. Then split the module into a new crate under `crates/<name>`.
2. Product code lives in `features/<name>/` with exactly `mod.rs http.rs service.rs repo.rs model.rs`. Infra stays top level. CI checks the shape.
3. Direction is `http → service → repo`. `http.rs` never imports `sqlx`; `repo.rs` never imports `axum`. CI greps for it.
4. All fallible code returns `crate::error::Result<T>`. `AppError` is the only error type that crosses a feature boundary. Startup code (`config`, `http::serve`, `cli`) keeps its own error types, which map to exit codes. `anyhow` only in `main.rs`, `cli.rs`, `build.rs`, and as the payload built for `AppError::Internal` (`anyhow!`, `.context()`).
5. `unwrap`, `expect`, `dbg!`, `todo!`, `println!`, `eprintln!` are clippy-denied. `clippy.toml` allows all but `todo!` in tests. `?` and `tracing` instead. The one exception: `cli.rs` prints command output to stdout under `#[expect(clippy::print_stdout, reason = "CLI output")]`.
6. All SQL lives in `repo.rs`, through `sqlx` macros, offline data committed in `.sqlx/`. Migrations are forward-only `.sql` files applied at startup.
7. Every path comes from `config::Paths`. No `~` or hardcoded dirs anywhere else.
8. `std::env::var` appears only in `config.rs`.
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

`just setup` installs or reports each one: rustup and the pinned toolchain, cargo-binstall, cargo-nextest, cargo-deny, cargo-machete, sqlx-cli, bacon, prek, bun, Node 24, actionlint. Cargo tools come from `cargo binstall`, falling back to `cargo install`. macOS uses Homebrew for the rest; Linux uses official installers or release binaries. It ends with `bun install` at the root and `prek install`, which installs the `pre-commit` and `commit-msg` hooks.

- `just` ≥ 1.42, enforced at the top of the `justfile`.
- `bacon.toml` holds the watch jobs. `just dev` runs `run-server`.
- `prek.toml` holds the commit hooks. commitlint and its conventional config live in a prek-managed node env, not in `package.json`. actionlint, cargo, and `just` come from the system.
- `renovate.json` opens grouped non-major updates weekly, plus lock file maintenance. Regex custom managers bump the pins in `prek.toml`. Automerge is off.
- `deny.toml` allows MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, MPL-2.0, Zlib, and crates.io as the only source.

## Dependency allowlist

Rust: tokio, axum, tower, tower-http, hyper, serde, serde_json, sqlx, utoipa, utoipa-axum, utoipa-scalar, tracing, tracing-subscriber, tracing-appender, tracing-opentelemetry, opentelemetry, opentelemetry_sdk, opentelemetry-otlp, figment, dotenvy, thiserror, anyhow, problem_details, clap, clap_complete, directories, rust-embed, mime_guess, ulid, time, uuid, url. Dev: axum-test, insta, tempfile, pretty_assertions, tokio-test.

Web: react, react-dom, @tanstack/react-query, @tanstack/react-router, @tanstack/router-plugin, @hey-api/openapi-ts, zod, tailwindcss, shadcn and its vendored deps, class-variance-authority, clsx, tailwind-merge, lucide-react. Dev: vite, @vitejs/plugin-react, typescript, @biomejs/biome, vitest, @testing-library/react, @testing-library/user-event, msw, @playwright/test, @julr/vite-plugin-validate-env, knip, react-doctor, ts-reset (`@total-typescript/ts-reset`), @types/react, @types/react-dom.

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

`cli.rs` owns the clap definition and dispatch; `main.rs` only parses and maps errors to exit codes. `triage config show` prints every `Config` var and each resolved path, aligned, with values formatted through `Debug` so `Secret<T>` prints `[redacted]`. URL userinfo prints as `redacted`. `Config::entries` destructures `Config` exhaustively, so a new field must be added there to compile. `triage completions <bash|zsh|fish>` writes a script to stdout and runs before config loads. `triage logs path` prints the absolute path of today's JSONL file (UTC date), whether or not it exists yet; `telemetry::log_file` builds the name from the same prefix and suffix as the appender. `triage status` sends `GET /health` to `127.0.0.1:$TRIAGE_PORT` through `http::probe`, a hand-written HTTP/1.1 request over `tokio::net::TcpStream` with a 2 s timeout and a 16 KiB response cap, because no HTTP client crate is allowlisted. It prints `running vX.Y.Z on 127.0.0.1:<port>, up 3h12m` and exits 0, or prints `not running` and exits 1 when nothing answers or the answer is not a triage health body. `cli::run` returns the process `ExitCode`.

## Server

`triage serve` (`http::serve`) takes `Paths::lock_file` (`<data>/triage.lock`) with std `File::try_lock` before it binds; no pid is written and the OS drops the lock when the process dies. A held lock exits 1 with "another triage instance is using <data dir>". It binds `127.0.0.1:$TRIAGE_PORT` only; a port in use exits 1 naming the port and `TRIAGE_PORT`. Signal handlers are registered before the bind, so a signal during startup still drains. SIGINT or SIGTERM stops accepting, drains open requests for up to 10 s, then exits 0. Startup logs `listening` with version, address, data dir, and log file; exit logs `stopped`, also when the server fails after binding. `AppState` holds `Arc<Config>`, `Arc<Paths>`, and the start `Instant`. `GET /health` returns `{"status":"ok","version","uptime_s"}`. `http::middleware::trace` wraps the router: it sets `x-request-id` to a UUIDv7, opens the request span, and copies the id onto the response. An incoming id is kept when it is 1 to 128 URI-unreserved characters (letters, digits, `-._~`) starting with a letter or digit; otherwise it is replaced. The per-request event `response` (`status`, `latency_ms`) logs at debug, so under the default `TRIAGE_LOG_FILE_LEVEL` the JSONL file gets one line per request and stderr shows only the span's close line. A 5xx raises `response` to warn; a body or stream error logs `response failed` at error. Startup errors stay outside `AppError`: `http::serve` returns `http::Result` and `cli::Error` wraps `config::Error` and `http::Error`, each with its own exit code.

## Errors

Handlers return `crate::error::Result<T>` with `AppError`: `NotFound(String)` 404, `Validation(Vec<FieldError>)` 422, `Conflict(String)` 409, `PayloadTooLarge` 413, `Internal(anyhow::Error)` 500. Each renders as RFC 9457 `application/problem+json` through `problem_details`: `type` is `https://triage.studio/problems/<slug>`, `title`, `status`, `detail` (4xx only, so 5xx never leak internals), `errors` for validation. `Internal` logs `internal error` at error level with `error` recorded as `&dyn Error`, so both sinks show the whole chain. `http::middleware::problems` sits inside `trace`: it caps extractor bodies (`Bytes`, `String`, `Json`, `Form`) at 1 MiB (`error::BODY_LIMIT`), turns a handler panic into an `Internal` 500 with tower-http `CatchPanicLayer`, turns any plain-text 413 (axum's body-limit rejection) into a problem, and sets `instance` to the request id. It swaps only the body, so headers the handler set survive. `AppError::into_response` stores the `Problem` in the response extensions so that middleware can add `instance`. Unknown `/api` paths fall back to a `NotFound` problem; other unknown paths return a bare 404, left for the web UI.

## Observability

`tracing` is the instrumentation. Three sinks: stderr columnar for humans, JSONL file for agents, OTLP when `Config.otel_endpoint` is set. `telemetry::init(&Config, &Paths)` installs the subscriber in `cli::run` after `Paths::resolve`, so anything logged during resolution is lost; `config::log_data_dir_override` re-emits the override line afterwards. `cli::run` holds the returned guard until exit; dropping it stops file logging. Completions skip telemetry.

- stderr: filter `TRIAGE_LOG_LEVEL`, format `TRIAGE_LOG_FORMAT` (`columns`, `pretty`, `compact`). ANSI colour only when stderr is a TTY.
- `columns` (default) prints `HH:MM:SS.mmm LEVEL module  message  key=value…  duration  req=<id>`. Time is local, with the offset read per event so DST changes apply; it falls back to UTC. Module is the last segment of the target. Newlines in values print as `⏎` and other control characters as `\xNN`. Each `source()` of an event's error field adds an indented `└ ` line; the field must be recorded as `error = &err as &dyn Error`, since `%err` and `?err` lose the chain. A closing span prints its name, its fields, and its wall-clock lifetime since creation; span error fields get no `└ ` lines. `req=` comes from the event's own `request_id` or the nearest span that has one. A span below `TRIAGE_LOG_LEVEL` is invisible to this sink, so its `request_id` never reaches `req=`.
- JSONL: `<logs>/triage.<YYYY-MM-DD>.jsonl`, daily rotation, 7 files kept, lossless non-blocking writer, filter `TRIAGE_LOG_FILE_LEVEL`. Each line leads with `ts level target message`, then the event fields flattened, then `span` (innermost name), `request_id` (the event's own, else the nearest span that has one), `span_id`, `trace_id` (root span id). Ids are strings and unique only among live spans of one process. `ts` is UTC with 6 sub-second digits. `message` is always a string. An error field holds its `source()` chain joined by `: `. An event field named like a reserved key is written as `field.<name>`, prefixed again until the key is free. The file filter also gates spans, so a span below `TRIAGE_LOG_FILE_LEVEL` gives its events no span keys.
- OTLP: every opentelemetry crate is used only in `telemetry/otel.rs`, so version bumps touch one file. With no endpoint, no OTLP code runs. Otherwise it adds a `tracing-opentelemetry` layer (file filter) with a batch exporter over HTTP/protobuf to `<endpoint>/v1/traces`, resource `service.name=triage` and `service.version`. Building never connects. Logged endpoints have their userinfo redacted. The first failed export logs one warning; later failures are silent. Dropping the guard shuts the provider down, which flushes pending spans before the file writer stops; each export, retries included, gets 2 s and the shutdown 5 s. The exporter still reads the other `OTEL_EXPORTER_OTLP_*` vars itself (headers, compression).
- An unwritable logs dir or an invalid filter logs one warning and continues on stderr or the default filter.

Every HTTP request opens a span `http` with `method`, `path` (no query), `request_id`, and `status`, recorded on response. Events inside a handler inherit `request_id`. A future `agent_run` span carries `session_id`, `model`, `cost_usd`, `num_turns`.

## Quality gates

| layer | trigger | runs |
|---|---|---|
| edit hook | Claude Code PostToolUse on Edit/Write | `cargo fmt` or `biome check --write` on that file |
| prek | commit | fmt, clippy, layout check, biome, tsc, react.doctor, schema and openapi drift, builtin file checks, actionlint, commitlint |
| `just check` | agent before PR, human anytime | prek set + nextest, vitest, knip, cargo-deny, cargo-machete, sqlx offline check |
| CI | PR | `just check` split into jobs, ubuntu + macos for nextest |
