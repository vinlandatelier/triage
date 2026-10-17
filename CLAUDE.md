# Triage

Local, single-binary life-admin agent. Rust daemon + CLI, React web UI, Discord
as the mobile surface. Claude Code is the only model runtime, driven as a
subprocess with the user's own login. No API keys.

One crate, `crates/triage`, builds the `triage` binary. It serves the API and,
in release, the embedded UI at `localhost:7311`. `web/` is a Vite SPA that talks
to the API through a client generated from `crates/triage/openapi.json`.
`docs/ARCHITECTURE.md` holds every rule and invariant; read it before a change.

## Commands

Fresh clone: `just setup && just check`. CI runs the same recipes.

| command | when |
|---|---|
| `just setup` | first clone, or after a dev tool changes; safe to re-run |
| `just dev` | serve the app at `localhost:7311` (Vite) with the daemon on 7312 |
| `just check` | before every push; every gate below except `e2e` |
| `just fix` | apply `cargo fmt` and Biome fixes |
| `just fmt-check` / `clippy` / `check-layout` / `test` | one Rust gate on its own; `test` is nextest |
| `just deny` / `machete` | after adding or removing a Rust dependency |
| `just check-web` | every web gate: lint, typecheck, doctor, knip, test |
| `just e2e` | Playwright against a fresh release build, after a UI flow changes |
| `just build-web` / `build` | `web/dist/`, then the release binary with it embedded |
| `just openapi` / `openapi-check` | after a route or response type changes |
| `just gen-api` / `gen-api-check` | after `openapi.json` changes, regenerate `web/src/api/` |
| `just db-prepare` / `db-prepare-check` | after a query or migration changes, regenerate `.sqlx/` |
| `just db-schema` / `db-schema-check` | after a migration, regenerate `schema.sql` (run `db-prepare` first) |
| `just skills-update` | pull upstream changes into vendored skills; never commits |
| `just loop <feature>` | the maintainer's private feature loop; `.eng/` is gitignored |
| `just` (`default`) | list recipes |
| `bun run dev` / `build` | Vite dev server / production build; `just dev` and `just build-web` call them |
| `bun run lint` / `fix` | Biome check / Biome write |
| `bun run typecheck` / `doctor` / `knip` / `test` | tsc, react-doctor, unused code, Vitest once |
| `bun run e2e` | Playwright only; needs `target/release/triage`, so prefer `just e2e` |
| `bun run gen:api` | same as `just gen-api`; runs `gen` in `tools/gen-api` |

Root scripts forward to the same names in `web/package.json`.

## Layout

```
crates/triage/src/          main.rs cli.rs config.rs error.rs telemetry http store
crates/triage/src/features/ one dir per feature: mod http service repo model
crates/triage/migrations/   forward-only SQL
crates/triage/tests/api/    integration tests, one file per feature
web/src/routes/             file-based routes, thin: loader + feature component
web/src/features/<name>/    feature UI and its tests
web/src/api/                generated client, never edited
web/e2e/                    Playwright specs
tools/gen-api/              hey-api generator (TypeScript 6)
scripts/                    check-layout.sh claude-format.sh
.claude/skills/             agent skills, see SOURCES.md
```

Copy `features/hello/` on both sides for a new feature.

## Where to look

- Logs: `triage logs path` prints today's JSONL file (`~/Library/Logs/triage/` on macOS). Every line carries `request_id`.
- Data: `triage db sql "<query>"` (read-only, `--json` for rows as objects). `triage db schema` prints the live schema.
- Schema: `crates/triage/schema.sql`. Read it instead of replaying migrations.
- API: `crates/triage/openapi.json`, or Scalar at `/api/scalar` in a debug build.
- Config: `triage config show` prints every `TRIAGE_*` var and path.
- Rules, invariants, and why: `docs/ARCHITECTURE.md`.

## Rules

- Rust in `crates/triage`: follow the `rust-service-conventions` skill.
- TypeScript in `web/`: follow the `web-conventions` skill.
- `docs/ARCHITECTURE.md` wins over any skill. Update it in the PR that adds a module, invariant, or dependency.
- Tooling changes update `just setup` in the same PR.
- No dependency outside the allowlist in `docs/ARCHITECTURE.md` without asking.
- Never hand-edit `web/src/api/`, `routeTree.gen.ts`, `openapi.json`, `schema.sql`, or `.sqlx/`.

## Simplicity

- No trait with a single implementation.
- No `utils` module. Name the module after what it does.
- No wrapper that only forwards a call.
- No config for a value that never varies.
- No generic before the second case exists.
- Delete before abstracting.
- Prefer a function to a one-method struct.
- Prefer std to a dependency.

## Conventions

- Commits and PR titles: Conventional Commits, `type(scope): imperative summary`.
- Feature work lands on `feature/<name>`: one PR, one commit per slice, in
  order, with a `Slice: <nn>-<slug>` trailer. The commit body carries a
  paragraph, `verify:` steps, and `notes:`; the PR body is the feature's PRD.
  Rebase-merge, so master gets one commit per slice.
- A slice commit is never amended or rebased while the PR is open. A review
  fix is a new commit titled `fixup! <slice subject>`; `/vai squash` folds them
  before the merge.
- Chores outside the loop use `<type>-<slug>` branches and squash-merge, one
  logical change per PR.
- Never push to `master`, merge, force-push, or rebase a pushed branch. The
  maintainer merges. `scripts/squash.sh` is the one sanctioned rewrite.
- PR body for a chore: `## what` (bullets), `## verify` (numbered steps), and
  `## why` / `## notes` only when needed. Bullets over paragraphs.
- Comments explain non-obvious why only. No restating code, no banners.
- A `just` recipe longer than about 10 lines moves to `scripts/<recipe>.sh`; the
  recipe calls it.
- Documentation names no individual people. Use "the maintainer" or "the user".
- When a question needs a person, mention whoever has the most context: the PR
  author, a reviewer, the author of the conflicting change. Mention
  only people with write access, never outside contributors or the public:
  `gh api repos/{owner}/{repo}/collaborators/<login>/permission` must say
  `admin`, `maintain`, or `write`. (`author_association` reads `CONTRIBUTOR`
  for members whose org membership is private.) When no one qualifies, mention
  the maintainer.

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
6. `scripts/walkthrough.sh <branch-or-pr> <video>` converts it to a GIF, hosts
   it on `refs/walkthroughs/<key>`, and prints the URL. The loop puts it in the
   slice commit's `Walkthrough:` trailer; a person puts it under
   `## walkthrough` in the PR body.
7. Commit none of the temporary spec, config, or video.

## AI review

Every push to a same-repo PR gets a review from the assistant, following
`.claude/skills/pr-review/SKILL.md`. It reviews only the commits that are new
since its last review, so a push that only squashes fixups reviews nothing.
Every finding is an inline comment, nits included, in the shape the skill
defines. Anything that needs a person is a `❓` decision; agents fixing review
comments never act on a `❓` comment.

## Asking for a decision

Any comment that needs a person's decision mentions whoever has the most
context (see Conventions) and uses this shape, scannable on a phone in five
seconds:

```
@<person> **your call:** <the question, one line>

`file:line` · <what is wrong or undecided, one line>

- **A (rec):** <option>. <why, one short clause>
- **B:** <option>. <tradeoff, one short clause>

Reply `A` or `B` in this thread.
```

Context is one line at most, each option one line, exactly one `(rec)`. No prose
paragraphs. Post it as an inline review comment on the line it is about, one
thread per decision, so the person replies in the thread. The assistant
answers any reply in a thread it started, no mention needed. Top-level only
when no diff line fits.

## Review assistant

Triggered by `@vinland-ai`, `/vinland-ai`, or `/vai` in a comment, or by any
reply in a thread it started. Never merge, never touch `master`.

- `/vai fix`: one session over every unresolved thread on the PR that is not a
  `❓`. For each: fix it, reply with one line, resolve the thread; or dispute
  with evidence (`file:line` or a repro) and leave it open. On a `feature/*`
  branch each slice touched gets one `fixup! <slice subject>` commit; elsewhere
  a normal commit. Run the relevant checks before pushing.
- `/vai squash`: a script, not the model. Folds `fixup!` commits into their
  slices on top of `origin/master`, pushes with `--force-with-lease`, replies
  "ready to merge". A conflict aborts and names the slice. Post it as a PR
  comment, not inline; from an inline thread the assistant only replies
  "post `/vai squash` as a PR comment".
- Anything else: make the requested change on the PR branch, run the relevant
  checks, push (`fixup!` on a feature branch), and reply in the same thread.
- A reply in a `❓` thread (`A`, `B`, or free text) is applied the same way.

Reply style. The reader is scanning on a phone between other work.

- Lead with the outcome in one line: what changed, or the answer.
- One idea per sentence, under 20 words, active voice, named actor.
- One term per concept. Concrete verbs. No hedging, no preamble, no closing line.
- Bullets for parallel points, numbered lists for sequences. Paragraphs ≤ 2 sentences.
- `file:line` for every code reference. Code and commands in fenced blocks.
- Reviews: each finding in the comment shape from
  `.claude/skills/pr-review/SKILL.md`. Skip strengths, context, and anything
  that is fine.
- Hard cap: 8 lines unless the user asked for detail. If more is needed, give
  the 8 lines and offer to expand.
- Ask at most one question, and only when the request is ambiguous.
