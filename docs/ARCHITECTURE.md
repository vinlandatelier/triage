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
      main.rs            clap → telemetry → run()
      lib.rs
      cli.rs             subcommands
      config.rs          env → Config, Paths
      error.rs           AppError → problem+json
      telemetry.rs       tracing layers, dormant otel
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
4. All fallible code returns `crate::error::Result<T>`. `AppError` is the only error type that crosses a feature boundary. `anyhow` only in `main.rs`, `cli.rs`, `build.rs`.
5. `unwrap`, `expect`, `dbg!`, `todo!`, `println!`, `eprintln!` are clippy-denied. `clippy.toml` allows all but `todo!` in tests. `?` and `tracing` instead.
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

`--data-dir <path>` or `TRIAGE_DATA_DIR` puts everything under one directory. Tests always use it.

## Observability

`tracing` is the instrumentation. Three sinks: stderr columnar for humans, JSONL file for agents, OTLP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set. Every HTTP request opens a span with `request_id`, `method`, `path`, `status`. A future `agent_run` span carries `session_id`, `model`, `cost_usd`, `num_turns`.

## Quality gates

| layer | trigger | runs |
|---|---|---|
| edit hook | Claude Code PostToolUse on Edit/Write | `cargo fmt` or `biome check --write` on that file |
| prek | commit | fmt, clippy, layout check, biome, tsc, react.doctor, schema and openapi drift, builtin file checks, actionlint, commitlint |
| `just check` | agent before PR, human anytime | prek set + nextest, vitest, knip, cargo-deny, cargo-machete, sqlx offline check |
| CI | PR | `just check` split into jobs, ubuntu + macos for nextest |
