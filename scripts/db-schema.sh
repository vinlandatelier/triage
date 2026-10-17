#!/usr/bin/env bash
# Regenerates api/schema.sql from a freshly migrated database; with --check, fails with a diff instead.
set -euo pipefail

cd "$(dirname "$0")/.."
out=api/schema.sql
main="$PWD/api/src/main.ts"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
# Bun reads `.env` from its working dir, and the caller's TRIAGE_* or OTEL_* vars could reach an
# exporter, so run in the temp dir with only HOME and PATH.
(cd "$dir" && env -i HOME="$HOME" PATH="$PATH" bun "$main" --data-dir "$dir/data" db schema) > "$dir/schema.sql"
if [ "${1:-}" = "--check" ]; then
    if ! diff -u "$out" "$dir/schema.sql"; then
        echo "$out is stale: run 'just db-schema' (-: committed, +: migrations)" >&2
        exit 1
    fi
else
    mv "$dir/schema.sql" "$out"
fi
