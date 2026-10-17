#!/usr/bin/env bash
# walkthrough.sh <key> <video>: turn a recording into a GIF, host it on refs/walkthroughs/<key>,
# print its raw URL. <key> is a branch name or PR number. The caller embeds the URL: the loop
# in a commit trailer, a person in the PR body. Clones and CI checkouts fetch only branches
# and tags, so GIFs on that ref never reach them.
set -euo pipefail
key=${1//\//-} video=$2
repo=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
if ! command -v ffmpeg >/dev/null; then
  # Installed here, not in the workflow, so runs that record nothing skip the install.
  if command -v apt-get >/dev/null; then sudo apt-get update -qq && sudo apt-get install -qq -y ffmpeg
  else echo "walkthrough.sh: install ffmpeg first (brew install ffmpeg)" >&2; exit 1; fi
fi
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
gif=$tmp/walkthrough.gif

for w in 960 800 640 480; do
  ffmpeg -loglevel error -y -i "$video" \
    -vf "fps=12,scale=$w:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5" "$gif"
  size=$(wc -c <"$gif")
  (( size < 10000000 )) && break
done
(( size < 10000000 )) || { echo "gif still ${size}B at 480px; record a shorter run" >&2; exit 1; }

# Plumbing instead of a checkout: no worktree and no local branch. Each recording is a new
# file on top of the ref, so earlier PR bodies keep working and the push never needs force.
ref=refs/walkthroughs/$key
path="$key/$(date +%s).gif"
for attempt in 1 2 3; do
  rm -f "$tmp/idx"; base=""
  if git fetch -q origin "$ref" 2>/dev/null; then base=$(git rev-parse FETCH_HEAD); GIT_INDEX_FILE=$tmp/idx git read-tree "$base"; fi
  blob=$(git hash-object -w "$gif")
  GIT_INDEX_FILE=$tmp/idx git update-index --add --cacheinfo "100644,$blob,$path"
  tree=$(GIT_INDEX_FILE=$tmp/idx git write-tree)
  commit=$(git commit-tree "$tree" ${base:+-p "$base"} -m "walkthrough for $key")
  git push -q origin "$commit:$ref" 2> >(grep -v "^remote:" >&2) && break
  [[ $attempt -eq 3 ]] && exit 1
done

echo "https://raw.githubusercontent.com/$repo/$commit/$path"
