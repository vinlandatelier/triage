#!/usr/bin/env bash
# Claude Code PostToolUse hook: formats the file an Edit or Write just touched.
# Always exits 0 so a formatter problem never blocks the edit.
set -uo pipefail

root="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"

# sed instead of jq, which `just setup` does not install. Paths with escaped quotes are not handled.
input="$(cat)"
field() { printf '%s' "$input" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -n1; }
file="$(field file_path)"
session="$(field session_id)"
[ -n "$file" ] && [ -f "$file" ] || exit 0

# One warning per tool per session, as JSON so Claude Code shows it without treating it as an error.
warn() {
    local marker="${TMPDIR:-/tmp}/triage-claude-format-${session:-none}-$1"
    [ -e "$marker" ] && return
    : >"$marker"
    printf '{"systemMessage":"claude-format: %s not found, run just setup"}\n' "$1"
}

case "$file" in
*.rs)
    # rustfmt on the one file: `cargo fmt` would reformat the whole workspace on every edit.
    if command -v rustfmt >/dev/null 2>&1; then
        (cd "$root" && rustfmt "$file") >/dev/null 2>&1
    else
        warn rustfmt
    fi
    ;;
"$root"/web/*.ts | "$root"/web/*.tsx | "$root"/web/*.js | "$root"/web/*.jsx | "$root"/web/*.json | "$root"/web/*.css)
    # The local binary, not bunx, which would download biome when it is missing.
    biome="$root/web/node_modules/.bin/biome"
    if [ -x "$biome" ]; then
        (cd "$root/web" && "$biome" check --write --no-errors-on-unmatched --files-ignore-unknown=true "$file") >/dev/null 2>&1
    else
        warn biome
    fi
    ;;
esac
exit 0
