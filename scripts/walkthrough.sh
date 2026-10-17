#!/usr/bin/env bash
# walkthrough.sh <pr> <video>: turn a recording into a GIF, host it in the <repo>-assets repo, embed it in the PR body.
# The GIFs live in their own repo so clones and CI checkouts of this one never download them.
set -euo pipefail
pr=$1 video=$2
repo=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
assets=${ASSETS_REPO:-$repo-assets}
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

# A new file per recording: no sha to look up, and parallel lanes never collide.
python3 - "$gif" "$tmp/put.json" "$pr" <<'PY'
import base64, json, sys
gif, out, pr = sys.argv[1:]
json.dump({"message": f"walkthrough for #{pr}", "content": base64.b64encode(open(gif, "rb").read()).decode()}, open(out, "w"))
PY
path="pr/$pr/$(date +%s).gif"
commit=$(gh api -X PUT "repos/$assets/contents/$path" --input "$tmp/put.json" --jq .commit.sha)

url="https://raw.githubusercontent.com/$assets/$commit/$path"
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
