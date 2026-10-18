#!/usr/bin/env bash
# Enforces rules 2, 3, and 8 of docs/ARCHITECTURE.md on api/src. Prints `file: rule <n>` per violation.
set -uo pipefail

cd "$(dirname "$0")/.." || exit
src=api/src
features="$src/features"
expected="http.ts model.ts repo.ts service.ts"
failed=0

violation() {
    echo "$1: rule $2"
    failed=1
}

# Prints matching lines, ignoring `//` comments so docs can name the forbidden items.
# A `//` right after `:` is kept, so a URL in a string does not hide the code before it.
code_grep() {
    sed -E 's:(^|[^:])//.*$:\1:' "$2" | grep -nE "$1"
}

# bun test writes `<name>.test.ts` snapshots to `__snapshots__/<name>.test.ts.snap`, with nothing else there.
snapshots() {
    [ -d "$1" ] && [ "$(basename "$1")" = __snapshots__ ] || return 1
    local snap
    for snap in "$1"/*; do
        [ -f "$snap" ] && [[ "$(basename "$snap")" == *.test.ts.snap ]] || return 1
    done
}

for dir in "$features"/*/; do
    [ -d "$dir" ] || continue
    dir="${dir%/}"
    for file in $expected; do
        [ -f "$dir/$file" ] || violation "$dir/$file" 2
    done
    for entry in "$dir"/*; do
        name="$(basename "$entry")"
        case " $expected " in
            *" $name "*) ;;
            *) [ -f "$entry" ] && [[ "$name" == *.test.ts ]] || snapshots "$entry" || violation "$entry" 2 ;;
        esac
    done
    if [ -f "$dir/http.ts" ] && code_grep "[\"'](drizzle-orm(/[^\"']*)?|bun:sqlite)[\"']" "$dir/http.ts" >/dev/null; then
        violation "$dir/http.ts" 3
    fi
    if [ -f "$dir/repo.ts" ] && code_grep "[\"'](hono|@hono/[^\"']*)(/[^\"']*)?[\"']" "$dir/repo.ts" >/dev/null; then
        violation "$dir/repo.ts" 3
    fi
done

[ -d "$src" ] || exit "$failed"
while IFS= read -r file; do
    if [ "$file" != "$src/config.ts" ] && code_grep '(^|[^[:alnum:]_$.])((process|Bun)\.env|import\.meta\.env|process\[["'"'"'"]env)([^[:alnum:]_$]|$)' "$file" >/dev/null; then
        violation "$file" 8
    fi
    if [ "$file" != "$src/cli.ts" ] && code_grep '(^|[^[:alnum:]_$.])(console\.|process\.stdout([^[:alnum:]_$]|$))' "$file" >/dev/null; then
        violation "$file" 8
    fi
done < <(find "$src" -name '*.ts' | sort)

exit "$failed"
