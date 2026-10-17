# Skill sources

Every skill in this directory is vendored from a GitHub repo, pinned to the commit below, or written here (`local`). `just skills-update` reads this table and skips `local` rows.

| skill | repo | path | commit | local edits |
|---|---|---|---|---|
| codebase-design | mattpocock/skills | skills/engineering/codebase-design | 49dd158d1076134a641b33efb035946536778336 | none |
| tdd | mattpocock/skills | skills/engineering/tdd | 49dd158d1076134a641b33efb035946536778336 | none |
| router-core | TanStack/router | packages/router-core/skills/router-core | caaf32da35e250c1344a350fdd80cfc631c24b3c | none |
| react-router | TanStack/router | packages/react-router/skills/react-router | caaf32da35e250c1344a350fdd80cfc631c24b3c | none |
| router-query | TanStack/router | packages/react-router/skills/compositions/router-query | caaf32da35e250c1344a350fdd80cfc631c24b3c | none |
| tanstack-query-best-practices | deckardger/tanstack-agent-skills | skills/tanstack-query | b2fc14c974b2ca09b38de09c28237f240175205b | none |
| shadcn | shadcn-ui/ui | skills/shadcn | d2fba50ee5b9113008b5bb908563527b7efd2f27 | none |
| vercel-composition-patterns | vercel-labs/agent-skills | skills/composition-patterns | 063bee94c3f4df8453406c830b0a7df0f2860278 | Vite SPA note in `SKILL.md` |
| vercel-react-best-practices | vercel-labs/agent-skills | skills/react-best-practices | 063bee94c3f4df8453406c830b0a7df0f2860278 | Vite SPA note in `SKILL.md` |
| rust-best-practices | neoeinstein/claude-plugins | plugins/rust-best-practices/skills/rust-best-practices | 89fdbf3f47122d975ac9ded802b3c14d85909aee | `AppError` instead of `color_eyre`, unwrap/expect denied everywhere, examples rewritten without `.unwrap()`/`.expect()`, facet and aliri_braid lines removed |
| rust-skills | leonardomso/rust-skills | . | fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9 | trimmed to `err- type- api- async- proj- name- test- obs- serde- lint- pat- anti-`, repo files and `checks/` dropped, see-also links to dropped rules removed, index regenerated, Cargo profile section dropped (`panic = "abort"` breaks `CatchPanicLayer`), `err-anyhow-app`, `err-no-unwrap-prod`, and `err-expect-bugs-only` summaries rewritten to match docs/ARCHITECTURE.md rules 4 and 5 |
| rust-service-conventions | local | - | - | written here, from `features/hello/` |
| web-conventions | local | - | - | written here, from `web/src/features/hello/` |
| api-conventions | local | - | - | written here; `api/src/features/hello/` becomes its worked example |
| pr-review | local | - | - | written here, for the ai-review workflow |

## Updating

`just skills-update` clones each repo at HEAD and applies the upstream diff since the pinned commit onto the vendored copy, then bumps the commit here. It never commits. The diff skips files that were upstream at the pinned commit but are gone here, so dropped files never conflict. A patch that hits a local edit leaves `.rej` files and keeps the old commit; resolve by hand. Files added upstream since the pin still arrive: after updating `rust-skills`, delete any new rule outside the kept prefixes and regenerate its index with upstream `checks/gen_index.py`, limited to those prefixes.

To add a skill: `npx skills add <repo> -s <name> -a claude-code --copy -y` (add `--full-depth` for skills nested under `packages/`), delete the generated `skills-lock.json`, and add a row here.
