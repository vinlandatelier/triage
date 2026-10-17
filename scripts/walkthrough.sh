#!/usr/bin/env bash
# walkthrough.sh <pr> <video>: turn a recording into a GIF, host it on the pr-assets branch, embed it in the PR body.
set -euo pipefail
pr=$1 video=$2
repo=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
command -v ffmpeg >/dev/null || { echo "walkthrough.sh: ffmpeg is not installed" >&2; exit 1; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
gif=$tmp/walkthrough.gif

for w in 960 800 640 480; do
  ffmpeg -loglevel error -y -i "$video" \
    -vf "fps=12,scale=$w:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5" "$gif"
  size=$(wc -c <"$gif")
  (( size < 10000000 )) && break
done
(( size < 10000000 )) || { echo "gif still ${size}B at 480px; record a shorter run" >&2; exit 1; }

# Plumbing instead of a checkout: no worktree, no local branch, so parallel lanes never collide.
for attempt in 1 2 3; do
  rm -f "$tmp/idx"; base=""
  if git fetch -q origin pr-assets 2>/dev/null; then base=$(git rev-parse FETCH_HEAD); GIT_INDEX_FILE=$tmp/idx git read-tree "$base"; fi
  blob=$(git hash-object -w "$gif")
  GIT_INDEX_FILE=$tmp/idx git update-index --add --cacheinfo "100644,$blob,pr/$pr/walkthrough.gif"
  tree=$(GIT_INDEX_FILE=$tmp/idx git write-tree)
  commit=$(git commit-tree "$tree" ${base:+-p "$base"} -m "chore: walkthrough for #$pr")
  git push -q origin "$commit:refs/heads/pr-assets" 2> >(grep -v "^remote:" >&2) && break
  [[ $attempt -eq 3 ]] && exit 1
done

# Commit-pinned URL so a re-recording never shows a cached old GIF.
url="https://raw.githubusercontent.com/$repo/$commit/pr/$pr/walkthrough.gif"
gh pr view "$pr" -R "$repo" --json body --jq .body >"$tmp/body.md"
python3 - "$tmp/body.md" "$url" <<'PY'
import re, sys
p, url = sys.argv[1], sys.argv[2]
body = open(p).read().rstrip()
section = f"## walkthrough\n![walkthrough]({url})"
if "## walkthrough" in body:
    body = re.sub(r"## walkthrough\n.*?(?=\n## |\Z)", section, body, flags=re.S)
else:
    body += "\n\n" + section
open(p, "w").write(body + "\n")
PY
gh pr edit "$pr" -R "$repo" --body-file "$tmp/body.md" >/dev/null
echo "$url"
