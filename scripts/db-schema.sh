#!/usr/bin/env bash
# Regenerates crates/triage/schema.sql from a freshly migrated database.
set -euo pipefail

cd "$(dirname "$0")/.."
out=crates/triage/schema.sql
cargo build --quiet --locked --bin triage
target="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
dir="$(mktemp -d)"
trap 'rm -rf "$dir" "$out.tmp"' EXIT
# A debug build reads `.env` from its working dir, and the caller's TRIAGE_* or OTEL_* vars
# could reach an exporter, so run in the temp dir with only HOME and PATH.
(cd "$dir" && env -i HOME="$HOME" PATH="$PATH" "$target/debug/triage" --data-dir "$dir" db schema) > "$out.tmp"
mv "$out.tmp" "$out"
