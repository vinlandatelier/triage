#!/usr/bin/env bash
# Pull each skill in .claude/skills/SOURCES.md to upstream HEAD for review. Never commits.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
sources="$root/.claude/skills/SOURCES.md"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
grep -E '^\| [a-z0-9-]+ \| [^ ]+ \| [^ ]+ \| [0-9a-f]{40} \|' "$sources" |
while IFS='|' read -r _ skill repo path pinned _; do
    # Table cells carry padding spaces; the values themselves never contain spaces.
    skill="${skill// /}" repo="${repo// /}" path="${path// /}" pinned="${pinned// /}"
    clone="$tmp/${repo//\//_}"
    [ -d "$clone" ] || git clone --quiet --filter=blob:none "https://github.com/$repo" "$clone"
    head="$(git -C "$clone" rev-parse HEAD)"
    relative=() prefix=""
    [ "$path" = "." ] || relative=("--relative=$path/") prefix="$path/"
    # Files that were upstream at the pin but are missing here were dropped on purpose; a diff
    # touching them could never apply.
    dropped=()
    while IFS= read -r file; do
        [ -e "$root/.claude/skills/$skill/${file#"$prefix"}" ] || dropped+=(":(exclude)$file")
    done < <(git -C "$clone" ls-tree -r --name-only "$pinned" -- "$path")
    git -C "$clone" diff --binary "${relative[@]}" "$pinned" "$head" -- "$path" "${dropped[@]}" > "$tmp/patch"
    if [ ! -s "$tmp/patch" ]; then
        printf '%-32s up to date\n' "$skill"
    # --reject keeps the hunks that apply and writes .rej files for the rest.
    elif git -C "$root" apply --reject --whitespace=nowarn --directory=".claude/skills/$skill" "$tmp/patch" 2>"$tmp/err"; then
        sed -i.bak "s/^| $skill \(|.*\)$pinned/| $skill \1$head/" "$sources" && rm "$sources.bak"
        printf '%-32s updated to %s\n' "$skill" "${head:0:12}"
    else
        printf '%-32s CONFLICT, resolve .rej files and bump the commit by hand\n' "$skill"
        sed 's/^/    /' "$tmp/err"
    fi
done
git -C "$root" status --short -- .claude/skills
