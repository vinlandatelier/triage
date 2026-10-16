# Triage

Local, single-binary life-admin agent. Rust daemon + CLI, React web UI, Discord
as the mobile surface. Claude Code is the only model runtime, driven as a
subprocess with the user's own login. No API keys.

## Conventions

- Commits and PR titles: Conventional Commits, `type(scope): imperative summary`.
- One issue, one PR, branch named `<issue>-<slug>`, body contains `Closes #<issue>`.
  Chores with no issue use `<type>-<slug>`.
- When a change depends on another unmerged PR, stack it with `gh stack` instead
  of waiting or bundling. Prefer a stack of small PRs over one large PR whenever
  the pieces are reviewable on their own.
- Never force push. Never push directly to `master`. Never merge; the maintainer merges.
- Squash merge is the repo policy, so keep each PR to one logical change.
- PR body: `Closes #N`, then `## what` (bullets), `## verify` (numbered steps),
  and `## why` / `## notes` only when needed. Bullets over paragraphs.
- Comments explain non-obvious why only. No restating code, no banners.
- Documentation names no individual people. Use "the maintainer" or "the user".

## Review assistant

When triggered from a PR comment: make the requested change on the PR branch,
run the relevant checks, push, and reply in the same thread. Never merge, never
force push, never touch `master`.

Reply style. The reader is scanning on a phone between other work.

- Lead with the outcome in one line: what changed, or the answer.
- One idea per sentence, under 20 words, active voice, named actor.
- One term per concept. Concrete verbs. No hedging, no preamble, no closing line.
- Bullets for parallel points, numbered list for sequences. No paragraphs over
  two sentences.
- `file:line` for every code reference. Code and commands in fenced blocks,
  not in prose.
- Reviews: one line per finding, severity first, defect then fix. Skip
  strengths, context, and anything that is fine.
- Hard cap: 8 lines unless the user asked for detail. If more is needed, give
  the 8 lines and offer to expand.
- Ask at most one question, and only when the request is ambiguous.
