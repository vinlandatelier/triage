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
- In a stack, change code from lower PRs only where your PR needs it. A defect
  in a lower PR gets fixed on that PR's branch, then restack.
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
- When a question needs a person, mention whoever has the most context: the PR
  or issue author, a reviewer, the author of the conflicting change. Mention
  only people with write access, never outside contributors or the public:
  `gh api repos/{owner}/{repo}/collaborators/<login>/permission` must say
  `admin`, `maintain`, or `write`. (`author_association` reads `CONTRIBUTOR`
  for members whose org membership is private.) When no one qualifies, mention
  the maintainer.

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
   authors of the conflicting changes: one line per conflict with file, options, and a recommendation.

   run `gh stack rebase --abort` and ask the maintainer on that PR, as in
   "Asking the maintainer" below.
4. Run the checks on each branch the rebase changed, then `gh stack push`.

Never merge one branch into another to catch up (`git merge`, `git pull`): a
merge commit stops `gh stack` from finding the old base for every PR above it.

## Walkthroughs

After pushing a change, decide whether a short recording would help the
reviewer see it: new or changed screens, interactions, visible states. Skip it
for anything a recording cannot show. When it helps, without being asked:

1. Write a Playwright spec that clicks through the change in under 20 seconds.
   Prefer an existing e2e spec. Record with video on and `slowMo` about 300 ms,
   via a temporary config that extends the repo's.
2. Install the browser first on a fresh machine or runner:
   `bunx playwright install --with-deps chromium`. If the branch has no
   Playwright yet, run it ad hoc (`bunx playwright` or `npx playwright`). Never
   add it as a dependency for a walkthrough.
3. Pace it for a human: type with `pressSequentially(text, { delay: 60 })`,
   not `fill`, and hold about 1 s on each result. The main success flow gets
   most of the runtime; error states come after it and stay short.
4. Caption every step with one line in a small fixed `<div>` at the top of the
   page, set through `page.evaluate` before the step runs. A viewer who never
   read the PR must follow it. Name any input the viewer cannot see: spaces, a
   stopped daemon, a mocked failure.
5. Check it before uploading: tile frames with
   `ffmpeg -i <video> -vf "fps=2,scale=480:-1,tile=4x4" -frames:v 1 frames.png`
   and look at them. Every step must show for at least two frames, and nothing
   may look broken that the PR does not intend. Otherwise re-record, or fix the
   app.
6. `scripts/walkthrough.sh <pr> <video>` converts it to a GIF, hosts it on
   `refs/walkthroughs/<pr>`, and embeds it under `## walkthrough` in the PR body.
7. Commit none of the temporary spec, config, or video.

## AI review

A PR labelled `request-ai-review` gets a review from the assistant on the label
and on every later push, following `.claude/skills/pr-review/SKILL.md`. Pushes
that only restack are skipped. Agents fixing review comments never act on a
`❓` comment; those wait for the maintainer.

## Asking the maintainer

Any comment that needs the maintainer's decision uses this shape, scannable on
a phone in five seconds:

```
@<maintainer> **your call:** <the question, one line>

`file:line` · <what is wrong or undecided, one line>

- **A (rec):** <option>. <why, one short clause>
- **B:** <option>. <tradeoff, one short clause>

Reply `A` or `B` in this thread.
```

Context is one line at most, each option one line, exactly one `(rec)`. No prose
paragraphs. Post it as an inline review comment on the line it is about, one
thread per decision, so the maintainer replies in the thread. The assistant
answers any reply in a thread it started, no mention needed. Top-level only
when no diff line fits.

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
