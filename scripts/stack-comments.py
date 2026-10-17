#!/usr/bin/env python3
"""Keep one stack comment on every stacked PR, Graphite-style: top of the stack first,
the PR itself marked, the base branch last. Rewrites every stack on each run, so a merge,
close, or retarget anywhere leaves no stale comment behind.

usage: stack-comments.py <owner/repo>   (GH_TOKEN must be the app's token)"""
import json, subprocess, sys
MARK = "<!-- stack -->"

def gh(*a, input=None):
    for i in range(5):
        r = subprocess.run(["gh", *a], capture_output=True, text=True, input=input)
        if r.returncode == 0: return r.stdout
    sys.exit(r.stderr)

def stacks(repo):
    """[(base branch, [PRs bottom to top])] for every chain of open PRs, singletons included."""
    prs = json.loads(gh("pr", "list", "-R", repo, "--state", "open", "--limit", "500",
                        "--json", "number,title,headRefName,baseRefName,isDraft"))
    by_head = {p["headRefName"]: p for p in prs}
    kids = {}
    for p in prs: kids.setdefault(p["baseRefName"], []).append(p)
    out = []
    for root in sorted((p for p in prs if p["baseRefName"] not in by_head), key=lambda p: p["number"]):
        order = []
        def walk(p):
            order.append(p)
            for k in sorted(kids.get(p["headRefName"], []), key=lambda k: k["number"]): walk(k)
        walk(root)
        out.append((root["baseRefName"], order))
    return out

def lines(base, order, here=None):
    rows = [f"- **#{p['number']}** 👈" if p["number"] == here else f"- #{p['number']}" for p in reversed(order)]
    return rows + [f"- `{base}`"]

def body(base, order, here):
    return "\n".join([MARK, "**Stack** · review and merge from the bottom up", "", *lines(base, order, here)]) + "\n"

def mine(repo, n):
    cs = json.loads(gh("api", f"repos/{repo}/issues/{n}/comments", "--paginate"))
    return [c for c in cs if c["user"]["type"] == "Bot" and c["body"].startswith(MARK)]

def upsert(repo, n, text):
    old = mine(repo, n)
    if old and old[0]["body"] == text: return
    if old: gh("api", "-X", "PATCH", f"repos/{repo}/issues/comments/{old[0]['id']}", "-F", "body=@-", input=text)
    else: gh("api", f"repos/{repo}/issues/{n}/comments", "-F", "body=@-", input=text)

def sync(repo):
    for base, order in stacks(repo):
        for p in order:
            if len(order) > 1: upsert(repo, p["number"], body(base, order, p["number"]))
            else:
                for c in mine(repo, p["number"]): gh("api", "-X", "DELETE", f"repos/{repo}/issues/comments/{c['id']}")

if __name__ == "__main__": sync(sys.argv[1])
