#!/usr/bin/env python3
"""Approvability verdict for a PR, in two halves.

  approvability.py gate <pr> <out.json>       hard rules, no model; waits for CI
  approvability.py post <pr> <verdict.json>   sticky verdict comment, then approve or dismiss

The gate fails closed: a rule it cannot check counts as failed. The model judges only
what the gate cannot (scope, tests, risk), and only after the gate passed.
GH_TOKEN must be the app's token, so the comment and approval come from the app."""
import json, os, re, subprocess, sys, time

MARK = "<!-- approvability -->"
MAX_LINES, MAX_FILES = 300, 10
BOT_LOGIN = "vinland-ai"
# Their size says nothing about review effort.
GENERATED = re.compile(
    r"(^|/)(Cargo\.lock|bun\.lock|openapi\.json|routeTree\.gen\.ts|schema\.sql)$|^\.sqlx/|^web/src/api/")
SENSITIVE = [
    (re.compile(r"^\.github/"), "changes CI or workflows"),
    (re.compile(r"(^|/)CLAUDE\.md$|^\.claude/"), "changes agent instructions"),
    (re.compile(r"(^|/)migrations/"), "changes the database schema"),
    (re.compile(r"(^|/)(Cargo\.toml|package\.json|Cargo\.lock|bun\.lock)$"), "changes dependencies"),
    (re.compile(r"(^|/)(deny\.toml|prek\.toml|renovate\.json|rust-toolchain\.toml)$"), "changes repo tooling policy"),
    (re.compile(r"auth|secret|token|credential|guardrail", re.I), "touches auth, secrets, or guardrails"),
]
BLOCKING = ("🔴", "🟠", "❓")
# Assistant automation jobs, not CI: their result says nothing about the code.
AUTOMATION_JOBS = {"review", "respond", "thread-reply", "sync", "restack"}


def gh(*args, input=None):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, input=input)
    if r.returncode:
        raise RuntimeError(f"gh {' '.join(args[:3])}: {r.stderr.strip()}")
    return r.stdout


def repo():
    return os.environ.get("GITHUB_REPOSITORY") or gh(
        "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner").strip()


def ci(r, sha, own, wait_min):
    """'green', 'red: <names>', 'pending', or 'none'. Skips `own`, which includes this still-running job."""
    deadline = time.time() + wait_min * 60
    while True:
        lines = gh("api", "--paginate", f"repos/{r}/commits/{sha}/check-runs",
                   "--jq", ".check_runs[] | {name, status, conclusion} | tojson").splitlines()
        runs = [json.loads(l) for l in lines if l and json.loads(l)["name"] not in own]
        if runs and all(x["status"] == "completed" for x in runs):
            bad = sorted({x["name"] for x in runs if x["conclusion"] not in ("success", "skipped", "neutral")})
            return f"red: {', '.join(bad)}" if bad else "green"
        if time.time() >= deadline:
            return "pending" if runs else "none"
        time.sleep(30)


def open_threads(r, pr):
    owner, name = r.split("/")
    q = """query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){pullRequest(number:$n){
      reviewThreads(first:100){nodes{isResolved path line originalLine comments(first:1){nodes{author{login} body}}}}}}}"""
    data = json.loads(gh("api", "graphql", "-F", f"o={owner}", "-F", f"r={name}", "-F", f"n={pr}", "-f", f"query={q}"))
    out = []
    for t in data["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]:
        if t["isResolved"] or not t["comments"]["nodes"]:
            continue
        c = t["comments"]["nodes"][0]
        bot = (c["author"] or {}).get("login") == BOT_LOGIN
        if not bot or c["body"].lstrip().startswith(BLOCKING):
            out.append(f"open {'review' if bot else 'human'} thread on `{t['path']}:{t['line'] or t['originalLine']}`")
    return out


def gate(pr, out, wait_min):
    r = repo()
    p = json.loads(gh("pr", "view", str(pr), "-R", r, "--json", "headRefOid,files,isDraft"))
    sha, files = p["headRefOid"], p["files"]
    passed, failed = [], []

    state = ci(r, sha, AUTOMATION_JOBS | {os.environ.get("GITHUB_JOB", "review")}, wait_min)
    if state == "green":
        passed.append("CI green")
    else:
        failed.append({"pending": f"CI still running after {wait_min} min",
                       "none": "no CI ran on this commit"}.get(state, f"CI {state}"))

    threads = open_threads(r, pr)
    failed += threads
    if not threads:
        passed.append("no open blocking review or human threads")

    if p["isDraft"]:
        failed.append("PR is a draft")

    # The API lists at most 100 files; a PR that hits the cap cannot be measured, so it fails.
    if len(files) >= 100:
        failed.append("100+ files, too large to check")
    real = [f for f in files if not GENERATED.search(f["path"])]
    lines = sum(f["additions"] + f["deletions"] for f in real)
    if lines > MAX_LINES or len(real) > MAX_FILES:
        failed.append(f"{lines} lines in {len(real)} files, over the {MAX_LINES}-line, {MAX_FILES}-file limit")
    else:
        passed.append(f"{lines} lines in {len(real)} files")

    for rx, why in SENSITIVE:
        hit = [f["path"] for f in files if rx.search(f["path"])]
        if hit:
            more = f" +{len(hit) - 1}" if len(hit) > 1 else ""
            failed.append(f"{why} (`{hit[0]}`{more})")

    result = {"sha": sha, "pass": not failed, "passed": passed, "failed": failed}
    with open(out, "w") as fh:
        json.dump(result, fh)
    print(json.dumps(result))
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as fh:
            fh.write(f"pass={'true' if not failed else 'false'}\nsha={sha}\n")


def post(pr, verdict_file):
    r = repo()
    v = json.load(open(verdict_file))
    # A gate result has no "approvable"; its failures are the reasons.
    ok = v.get("approvable", False) and v.get("pass", True)
    reasons = v.get("reasons") or v.get("failed") or ["no reasons given"]
    sha = v["sha"]
    head = f"{'✅ **Approvable**' if ok else '❌ **Not approvable**'} · `{sha[:7]}`"
    body = "\n".join([MARK, head, "", *[f"- {x}" for x in reasons]]) + "\n"

    mine = [c for c in json.loads(gh("api", "--paginate", f"repos/{r}/issues/{pr}/comments"))
            if c["user"]["type"] == "Bot" and c["body"].startswith(MARK)]
    for c in mine[1:]:
        gh("api", "-X", "DELETE", f"repos/{r}/issues/comments/{c['id']}")
    if mine:
        if mine[0]["body"] != body:
            gh("api", "-X", "PATCH", f"repos/{r}/issues/comments/{mine[0]['id']}", "-F", "body=@-", input=body)
    else:
        gh("api", f"repos/{r}/issues/{pr}/comments", "-F", "body=@-", input=body)

    approvals = [x for x in json.loads(gh("api", "--paginate", f"repos/{r}/pulls/{pr}/reviews"))
                 if x["user"]["login"] == f"{BOT_LOGIN}[bot]" and x["state"] == "APPROVED"]
    if ok:
        # One approval per approvable streak; the comment above carries the current SHA.
        if not approvals:
            gh("api", f"repos/{r}/pulls/{pr}/reviews", "-f", f"commit_id={sha}",
               "-f", "event=APPROVE", "-f", f"body={head}")
    else:
        # An approval from an earlier push must not outlive the change that made it wrong.
        for x in approvals:
            gh("api", "-X", "PUT", f"repos/{r}/pulls/{pr}/reviews/{x['id']}/dismissals",
               "-f", f"message=Not approvable at {sha[:7]}", "-f", "event=DISMISS")
    print(head)


if __name__ == "__main__":
    cmd, pr, path = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    if cmd == "gate":
        gate(pr, path, int(os.environ.get("CI_WAIT_MIN", "30")))
    elif cmd == "post":
        post(pr, path)
    else:
        sys.exit(f"unknown command {cmd}")
