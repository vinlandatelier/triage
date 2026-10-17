#!/usr/bin/env python3
"""Keep one stack comment on every stacked PR, Graphite-style: the stack top first, the PR
itself marked, the base branch last. Rewrites every stack on each run, so a merge, close,
or retarget anywhere leaves no stale comment behind.

usage: stack-comments.py <owner/repo> [closed PR number]   (GH_TOKEN must be the app's token)"""
import json, re, subprocess, sys, time
MARK = "<!-- stack -->"

class GhError(Exception): pass

def gh(*a, input=None):
    for i in range(5):
        r = subprocess.run(["gh", *a], capture_output=True, text=True, input=input)
        if r.returncode == 0: return r.stdout
        # Client errors will not fix themselves; rate limits and server errors might.
        m = re.search(r"HTTP (\d{3})", r.stderr)
        if m and m.group(1).startswith("4") and m.group(1) != "429": break
        time.sleep(5 * 2 ** i)
    raise GhError(f"gh {' '.join(a[:3])}: {r.stderr.strip()}")

def stacks(repo):
    """[(base branch, root PR, children by branch)] for every tree of open PRs, singletons included."""
    prs = json.loads(gh("pr", "list", "-R", repo, "--state", "open", "--limit", "500",
                        "--json", "number,title,headRefName,baseRefName"))
    by_head = {p["headRefName"]: p for p in prs}
    kids = {}
    for p in prs: kids.setdefault(p["baseRefName"], []).append(p)
    for k in kids.values(): k.sort(key=lambda p: p["number"])
    roots = sorted((p for p in prs if p["baseRefName"] not in by_head), key=lambda p: p["number"])
    return [(r["baseRefName"], r, kids) for r in roots]

def members(root, kids):
    out = [root]
    for k in kids.get(root["headRefName"], []): out += members(k, kids)
    return out

def paths(root, kids):
    """Every root-to-leaf chain. A branched tree becomes several chains sharing their lower PRs,
    so siblings never look stacked on each other."""
    ks = kids.get(root["headRefName"], [])
    return [[root]] if not ks else [[root] + rest for k in ks for rest in paths(k, kids)]

def lines(base, path, here=None):
    rows = [f"- **#{p['number']}** 👈" if p["number"] == here else f"- #{p['number']}" for p in reversed(path)]
    return rows + [f"- `{base}`"]

def body(base, chains, here):
    out = [MARK, "**Stack** · review and merge from the bottom up"]
    for c in chains: out += ["", *lines(base, c, here)]
    return "\n".join(out) + "\n"

def mine(repo, n):
    cs = json.loads(gh("api", f"repos/{repo}/issues/{n}/comments", "--paginate"))
    return [c for c in cs if c["user"]["type"] == "Bot" and c["body"].startswith(MARK)]

def drop(repo, cs):
    for c in cs: gh("api", "-X", "DELETE", f"repos/{repo}/issues/comments/{c['id']}")

def upsert(repo, n, text):
    old = mine(repo, n)
    drop(repo, old[1:])
    if old and old[0]["body"] == text: return
    if old: gh("api", "-X", "PATCH", f"repos/{repo}/issues/comments/{old[0]['id']}", "-F", "body=@-", input=text)
    else: gh("api", f"repos/{repo}/issues/{n}/comments", "-F", "body=@-", input=text)

def sync(repo, closed=None):
    failed = []
    def each(n, f):
        try: f()
        except GhError as e: failed.append(f"#{n}: {e}")
    # A closed PR is no longer listed, so clear its comment by number.
    if closed: each(closed, lambda: drop(repo, mine(repo, closed)))
    for base, root, kids in stacks(repo):
        ms = members(root, kids)
        for p in ms:
            n = p["number"]
            if len(ms) > 1: each(n, lambda: upsert(repo, n, body(base, [c for c in paths(root, kids) if p in c], n)))
            else: each(n, lambda: drop(repo, mine(repo, n)))
    if failed: sys.exit("\n".join(failed))

if __name__ == "__main__": sync(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2] else None)
