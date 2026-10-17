#!/usr/bin/env bash
# Enforces the layout rules in docs/ARCHITECTURE.md. Prints `file: rule` per violation.
set -uo pipefail

cd "$(dirname "$0")/.." || exit
src=crates/triage/src
features="$src/features"
expected="http.rs mod.rs model.rs repo.rs service.rs"
failed=0

violation() {
    echo "$1: $2"
    failed=1
}

# Prints matching lines, ignoring `//` comments so docs can name the forbidden items.
# A `//` right after `:` is kept, so a URL in a string does not hide the code before it.
code_grep() {
    sed -E 's:(^|[^:])//.*$:\1:' "$2" | grep -nE "$1"
}

for dir in "$features"/*/; do
    [ -d "$dir" ] || continue
    dir="${dir%/}"
    for file in $expected; do
        [ -f "$dir/$file" ] || violation "$dir" "missing $file"
    done
    for entry in "$dir"/*; do
        name="$(basename "$entry")"
        case " $expected " in
            *" $name "*) ;;
            *) violation "$entry" "feature dirs hold only $expected" ;;
        esac
    done
    if [ -f "$dir/http.rs" ] && code_grep '(^|[^[:alnum:]_])sqlx::|use sqlx' "$dir/http.rs" >/dev/null; then
        violation "$dir/http.rs" "http.rs must not use sqlx"
    fi
    if [ -f "$dir/repo.rs" ] && code_grep '(^|[^[:alnum:]_])axum::|use axum' "$dir/repo.rs" >/dev/null; then
        violation "$dir/repo.rs" "repo.rs must not use axum"
    fi
done

while IFS= read -r file; do
    if [ "$file" != "$src/config.rs" ] && code_grep '(^|[^[:alnum:]_])env::(var|\{([^}]*[^[:alnum:]_])?var)' "$file" >/dev/null; then
        violation "$file" "std::env::var only in config.rs"
    fi
    if [ "$file" != "$src/cli.rs" ] && code_grep '(^|[^[:alnum:]_])println!' "$file" >/dev/null; then
        violation "$file" "println! only in cli.rs"
    fi
done < <(find "$src" -name '*.rs' | sort)

exit "$failed"
