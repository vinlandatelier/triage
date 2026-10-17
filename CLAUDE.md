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
- Never push directly to `master`. Never merge; the maintainer merges.
- The only rewrite of a pushed branch is a restack: `gh stack rebase`, then
  `gh stack push` (`--force-with-lease`). Never plain `--force`.
- Stack operations go through `gh stack` only (`checkout`, `rebase`, `push`,
  `sync`, `link`), never `git rebase` or a hand-run `git push --force-with-lease`.
- Squash merge is the repo policy, so keep each PR to one logical change.
- PR body: `Closes #N`, then `## what` (bullets), `## verify` (numbered steps),
  and `## why` / `## notes` only when needed. Bullets over paragraphs.
- Comments explain non-obvious why only. No restating code, no banners.
- Documentation names no individual people. Use "the maintainer" or "the user".

## Restacking

A PR in a stack that changes, or a PR below it that merges, leaves the PRs above
it behind. Restack them:

1. `gh stack checkout <PR>`, then `gh stack rebase --upstack` (or `gh stack
   rebase` after a merge).
2. Resolve each conflict whose intent is clear, then `gh stack rebase --continue`:
   - same change on both sides: keep one copy
   - independent edits: keep both
   - generated or lock files: take either side, regenerate with `just`
   - otherwise keep what each PR's issue says that PR must do
3. If a resolution needs a product decision, or both intents might not survive,
   run `gh stack rebase --abort` and comment on that PR mentioning the
   maintainer: one line per conflict with file, options, and a recommendation.
4. Run the checks on each branch the rebase changed, then `gh stack push`.

Never merge one branch into another to catch up (`git merge`, `git pull`): a
merge commit stops `gh stack` from finding the old base for every PR above it.

## Review assistant

When triggered from a PR comment: make the requested change on the PR branch,
run the relevant checks, push, restack if the PR has PRs above it, and reply in
the same thread. Never merge, never touch `master`.

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
