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
run the relevant checks, push, and reply in the same thread. Replies are one to
three lines: what changed, or one precise question if the request is ambiguous.
Never merge, never force push, never touch `master`.
