#!/usr/bin/env bash
# squash.sh <feature-branch>: fold `fixup! <slice>` commits into their slices on top of
# origin/master and force-push. Prints "<n> fixup commits folded". A conflict aborts and
# names the slice, so a person resolves it; the script never guesses.
set -euo pipefail
branch=$1
[[ "$branch" == feature/* ]] || { echo "not a feature branch: $branch" >&2; exit 1; }
git fetch -q origin master "$branch"
git checkout -q -B "$branch" "origin/$branch"
n=$(git log --format=%s "origin/master..$branch" | grep -c '^fixup! ' || true)
if ! GIT_SEQUENCE_EDITOR=: git rebase -q -i --autosquash origin/master; then
  stopped=$(git log -1 --format=%s REBASE_HEAD 2>/dev/null || echo unknown)
  git rebase --abort
  echo "conflict in ${stopped#fixup! }" >&2
  exit 1
fi
git push -q --force-with-lease origin "$branch"
echo "$n fixup commits folded"
