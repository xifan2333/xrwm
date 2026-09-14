# Claude Code Guide

See [AGENTS.md](AGENTS.md) for the full architecture, coding guidelines, quality gates, and the mandatory Issue + Draft PR workflow.

## Quick CLI Reference

- **Dual-Planning Model**:
  - _Phase A (Task Planning - BEFORE CODING)_: `gh issue view <id>` -> `git checkout -b <type>/issue-<id>-<desc>` -> `git commit --allow-empty -m "chore: initialize draft pr for #<id>"` -> `git push -u origin <branch>` -> `gh pr create --draft --body "Closes #<id>\n\n### Tasks\n- [ ] 1. ..."`
  - _Phase B (Quality Gate Pre-check - AFTER EDIT)_: `mise run check:plan`
- **Single-Item Loop (ONE ITEM AT A TIME, PR STAYS DRAFT)**:
  - Code task N -> `mise run check:plan` -> `mise run check:changed` -> local atomic commit -> `git push` -> `gh pr edit --body-file` (tick that item `- [x]`, preserving the rest of the body) -> repeat
- **Finish (only when every item is ticked)**:
  - `gh pr ready` (awakens CodeRabbit & Greptile once on the complete diff) -> review-fix loop -> `gh pr merge --squash --delete-branch`
- **Quality Gate Tasks (`mise`)**:
  - `mise run check:plan`: preview the execution plan without running tools
  - `mise run check:changed`: run checks on modified/staged/untracked files
  - `mise run fix`: auto-format files
  - `mise run lint`: full-repository static analysis
  - `mise run build` / `mise run test`: compile and run the test suite

## Hard Constraints (Red Lines)

- **No Direct Main Commits**: always develop on a feature branch and open a Draft PR.
- **Sequential Checklist**: implement only the topmost unchecked item. Commit, push, and tick it before starting the next one.
- **No Mid-Round Reviews**: keep the PR in draft and never invoke CodeRabbit/Greptile until every checklist item is ticked. CodeRabbit reviews incrementally and will not re-review earlier commits.
- **Quality Gate Before Every Commit**: keep `mise run check:changed` green; never bypass hooks with `--no-verify` or suppress diagnostics with `#[allow]`/`-Wno-*`.
- **Panic & Unsafe Policy**: production code denies `unwrap()` (`clippy::unwrap_used`) and `unsafe` (`unsafe_code`), with no exceptions. Tests may use `unwrap()`; syscalls go through the safe `rustix` bindings.
- **Atomic Commits**: one commit per checklist item, squash-merged at the end.
