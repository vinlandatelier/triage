set shell := ["bash", "-euo", "pipefail", "-c"]

user_path := env("PATH")

# Installers put tools here, and brew's node@24 is keg-only, so a fresh shell would not find them yet.
export PATH := env("HOME") + "/.cargo/bin:" + env("HOME") + "/.bun/bin:" + env("HOME") + "/.local/bin:/opt/homebrew/opt/node@24/bin:/usr/local/opt/node@24/bin:" + env("PATH")

# just has no min-version setting. shell() rather than a backtick so --dry-run still evaluates it.
_min_just := if semver_matches(shell(quote(just_executable()) + ' --version | cut -d" " -f2'), ">=1.42.0") == "true" { "" } else { error("just >= 1.42.0 is required") }

dev_port := env("TRIAGE_DEV_PORT", "7311")

[private]
default:
    @{{ quote(just_executable()) }} --list

# Install every dev tool. Safe to re-run.
setup:
    @TRIAGE_USER_PATH={{ quote(user_path) }} ./scripts/setup.sh

# Run every quality gate.
check: fmt-check clippy check-layout deny machete db-prepare-check test check-web gen-api-check

# Check Rust formatting.
fmt-check:
    cargo fmt --all --check

# Lint Rust with warnings denied.
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Fail on any breach of the layout rules in docs/ARCHITECTURE.md.
check-layout:
    scripts/check-layout.sh

# Check licenses, advisories, bans, and sources.
deny:
    cargo deny check

# Find unused Rust dependencies.
machete:
    cargo machete

# Run the Rust tests.
test:
    cargo nextest run --workspace --locked --no-tests=pass

# Lint, typecheck, and test the web UI.
check-web:
    bun run lint
    bun run typecheck
    bun run doctor
    bun run knip
    bun run test

# Run the browser tests against a fresh release build.
e2e: build
    bun run e2e

# Build the web UI into web/dist.
build-web:
    bun run build

# Build the release binary with the web UI embedded.
build: build-web
    cargo build --release --locked

# Regenerate web/src/api, the typed client, from crates/triage/openapi.json.
gen-api:
    bun run gen:api

# Fail when web/src/api no longer matches crates/triage/openapi.json. Staged output passes.
gen-api-check: gen-api
    #!/usr/bin/env bash
    set -euo pipefail
    untracked="$(git ls-files --others --exclude-standard -- web/src/api)"
    if ! git diff --quiet -- web/src/api || [ -n "$untracked" ]; then
        git status --short -- web/src/api
        echo "web/src/api is stale: run 'just gen-api' and commit the result"
        exit 1
    fi

# Regenerate crates/triage/schema.sql from a freshly migrated database.
db-schema:
    scripts/db-schema.sh

# Fail when crates/triage/schema.sql no longer matches the migrations.
db-schema-check:
    cargo nextest run --workspace --locked -E 'test(=schema::schema_dump_is_current)'

# Regenerate crates/triage/.sqlx, the offline query data the sqlx macros compile against.
db-prepare:
    #!/usr/bin/env bash
    set -euo pipefail
    just _with-migrated-db cargo sqlx prepare -- --all-targets --locked

# Fail when crates/triage/.sqlx no longer matches the queries.
db-prepare-check:
    #!/usr/bin/env bash
    set -euo pipefail
    just _with-migrated-db cargo sqlx prepare --check -- --all-targets --locked

# sqlx-cli migrates instead of `triage db migrate`, which cannot build while .sqlx is stale.
[no-cd]
[private]
_with-migrated-db +cmd:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="$(mktemp -d)"
    trap 'rm -rf "$dir"' EXIT
    export DATABASE_URL="sqlite:$dir/triage.db"
    sqlx database create
    sqlx migrate run --source "{{justfile_directory()}}/crates/triage/migrations" >/dev/null
    cd "{{justfile_directory()}}/crates/triage"
    {{cmd}}

# Regenerate crates/triage/openapi.json from the registered routes.
openapi:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'rm -f crates/triage/openapi.json.tmp' EXIT
    cargo run --quiet --locked --bin gen_openapi > crates/triage/openapi.json.tmp
    mv crates/triage/openapi.json.tmp crates/triage/openapi.json

# Fail when crates/triage/openapi.json no longer matches the code.
openapi-check:
    cargo nextest run --workspace --locked -E 'test(=openapi::openapi_spec_is_current)'

# Pull each skill in .claude/skills/SOURCES.md to upstream HEAD for review. Never commits.
skills-update:
    @./scripts/skills-update.sh

# Run the maintainer's private feature loop (.eng/ is gitignored).
loop +args:
    python3 .eng/loop/run.py {{args}}

# Apply automatic fixes.
fix:
    cargo fmt --all
    bun run fix

# Serve the app at localhost:$TRIAGE_DEV_PORT (default 7311): Vite for the UI, proxying /api to the daemon on 7312.
dev: _dev-port-free _dev-run

# Vite binds localhost, which may resolve to ::1, so strictPort alone misses a daemon on 127.0.0.1.
[private]
_dev-port-free:
    #!/usr/bin/env bash
    set -euo pipefail
    port="{{dev_port}}"
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
        echo "installed triage is running on $port, stop it or set TRIAGE_DEV_PORT" >&2
        exit 1
    fi
    echo "triage dev: http://localhost:$port"

[parallel]
[private]
_dev-run: _dev-server _dev-web

[private]
_dev-server:
    TRIAGE_PORT=7312 bacon --headless run-server

[private]
_dev-web:
    cd web && bun run dev --port {{dev_port}}
