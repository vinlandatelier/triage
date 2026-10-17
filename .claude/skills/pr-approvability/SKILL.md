---
name: pr-approvability
description: Decide whether a PR is safe to approve without a human reading it. Use when the ai-review workflow runs the approvability step after the hard gate passed.
---

# PR approvability

`scripts/approvability.py gate` already checked the hard rules: CI is green, no
blocking or human threads are open, the PR is not a draft, the size is within the
limit, and no sensitive path is touched. You judge only what a script cannot.

Be skeptical. Approve only when a reviewer would be wasting their time reading
the PR. Any doubt means not approvable.

## Read

- The PR's own diff against its base branch: `gh pr diff <n>`. For a stacked PR,
  ignore the base PR's content.
- The linked issue (`Closes #N`).
- The tests the diff adds or changes, and the code they exercise.

## Approvable only if all hold

1. **Scope:** the diff does what the issue asks, and nothing else. No drive-by
   refactors, renames, or unrelated cleanups.
2. **Tests:** every changed behaviour has a test that would fail without the
   change. Pure docs, comments, or config with no behaviour need no test.
3. **Risk:** a bug here would be low impact and easy to revert. Not approvable
   when it changes data written to disk, error handling on a hot path, concurrency,
   public API or CLI shape, or anything the issue calls user-facing behaviour
   without a test pinning it.
4. **Clarity:** you understood every hunk. A hunk you cannot explain makes the
   PR not approvable.

## Output

Write exactly one JSON object to the path the workflow gives you, with a shell
heredoc. Do not post anything, edit files, or push.

```json
{"approvable": false, "reasons": ["one line each"]}
```

- `reasons`: 3 to 5 one-liners, each under 80 characters, no trailing period.
- Approvable: start with the gate's passed facts, then your own reasons, e.g.
  `stays within #38: adds the hello form, nothing else`.
- Not approvable: only the reasons it fails, most serious first, with
  `file:line` where one applies, e.g. `` `http.rs:40` retry loop has no test ``.
