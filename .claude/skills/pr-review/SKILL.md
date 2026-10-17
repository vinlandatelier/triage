---
name: pr-review
description: Review a pull request and post the findings as one GitHub review with inline comments. Use when asked to review a PR, or when the ai-review workflow runs.
---

# PR review

The analysis is meticulous. The output is compact: inline comments on the
lines, plus a summary of at most three lines.

## 1. Scope

- **Full review** (first run on the PR): the PR's own diff against its base
  branch, `gh pr diff <n>`. For a stacked PR, never re-review the base PR's content.
- **Incremental review** (a later push): only the commits the workflow lists
  as new. Code that did not change since your last review is out of scope.
- Read the linked issue (`Closes #N`). The first check is always: does the diff
  do what the issue says, nothing more, nothing less. In a stacked PR, flag any
  change to code from a lower PR that the issue does not need (🟠): it belongs
  on the lower PR's branch.
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

- 🔴 bug, security issue, data loss, or breaks the issue's intent
- 🟠 real design, correctness, or test gap
- 🟡 clarity or maintainability
- ⚪ `nit:` style or taste

## 5. Ask instead of guessing

Err towards asking. A finding becomes a decision for a person when any of
these holds:

- The fix touches security, auth, secrets, data deletion, migrations, or
  guardrail and autonomy semantics.
- The fix is hard to reverse.
- The issue or spec is vague or silent on the behaviour in question.
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

- Each finding is an inline comment on the line it is about:
  `<severity> <defect>. Fix: <fix>.` Add a GitHub `suggestion` block when the
  fix is a pure replacement of the commented lines.
- A finding goes in the top-level body only when no diff line can carry it.
- `body` is at most three lines: the counts (`🔴 0 · 🟠 1 · 🟡 2 · ⚪ 1`), then
  one line on what matters most. No preamble, no praise, no restating the PR.
- Nothing new found: post nothing at all.
