---
name: pr-review
description: Review a pull request and post the findings as one GitHub review with inline comments. Use when asked to review a PR, or when the ai-review workflow runs.
---

# PR review

The analysis is meticulous. The output is compact: inline comments on the
lines, plus a summary of at most three lines.

## 1. Scope

- **Full review** (first run on the PR): the PR's own diff against its base
  branch, `gh pr diff <n>`.
- **Incremental review** (a later push): only the commits the workflow lists
  as new. Code that did not change since your last review is out of scope.
- Read the PR body and, on a `feature/*` branch, each new commit's body: the
  slice's paragraph, `verify:` steps, and `notes:`. The first check is always:
  does the diff do what that says, nothing more, nothing less.
- Read the code around each hunk, its callers and tests, and grep for sibling
  patterns. Most real bugs sit at the seams between the diff and what it touches.

## 2. Earlier reviews

Before writing anything, read your own earlier comments on this PR:
`gh api repos/{owner}/{repo}/pulls/<n>/comments` and `.../reviews`, plus
`gh api graphql` `reviewThreads { isResolved }` for thread state.

- Never raise a point again that you, or anyone, already raised.
- Resolved threads are settled. Do not reopen them.
- An unresolved thread whose fix is wrong or partial gets a reply in that
  thread, not a new comment.

## 3. Lenses

Report a lens only when it has a finding.

- Functionality: edge cases, error paths, concurrency, data loss, security.
- Design: right layer, follows `docs/ARCHITECTURE.md`, abstractions earn their keep.
- Complexity: anything simpler, or deletable.
- Tests: the risky paths are covered, and the tests fail when the code breaks.
- Naming and comments: names are honest; comments state only non-obvious why.

Before a finding ships: grep the repo before calling something a convention
breach, and run a cheap repro when a claim of misbehaviour costs under a minute
to prove.

## 4. Severity

- 🔴 bug, security issue, data loss, or breaks the slice's or PR's intent
- 🟠 real design, correctness, or test gap
- 🟡 clarity or maintainability
- ⚪ `nit:` style or taste

## 5. Ask instead of guessing

Err towards asking. A finding becomes a decision for a person when any of
these holds:

- The fix touches security, auth, secrets, data deletion, migrations, or
  guardrail and autonomy semantics.
- The fix is hard to reverse.
- The PR or slice body is vague or silent on the behaviour in question.
- Two reasonable fixes lead to different product behaviour.

Write it as an inline comment starting with `❓`, in the "Asking for a decision"
format from `CLAUDE.md`, mentioning the PR author unless someone else has more context.
Agents that act on review comments never act on a `❓` comment.

## 6. Post

One review per run, `event: COMMENT`. Never `APPROVE` or `REQUEST_CHANGES`.

```
gh api repos/{owner}/{repo}/pulls/<n>/reviews --input review.json
```

`review.json`: `commit_id` (PR head), `event`, `body`, and `comments` with
`path`, `line`, `side: RIGHT`, `body`.

- Each finding except a `❓` decision (§5), nits included, is one inline
  comment on the line it is about, in exactly this shape:

  ````
  <severity from §4> **<the defect, at most 10 words>**

  <the concrete case that triggers it, one line, code in backticks>

  **Fix:** <the fix, one line>

  <details><summary>evidence</summary>

  <repro command and output, or the reasoning, only when there is some>
  </details>
  ````

  When the fix is a pure replacement of the commented lines, a `suggestion`
  block replaces the `**Fix:**` line.
- The visible part is at most four short lines. Everything that proves the
  finding goes in `evidence`, collapsed.
- No first person ("I confirmed", "I checked"), no restating the diff, no
  "this means", no closing remark. Name files as `path:line`.
- One finding per comment. A second problem on the same line is a second
  comment.
- A finding goes in the top-level body only when no diff line can carry it.
- `body` is at most three lines: the counts (`🔴 0 · 🟠 1 · 🟡 2 · ⚪ 1`), then
  one line on what matters most. No preamble, no praise, no restating the PR.
- Nothing new found: post nothing at all.
