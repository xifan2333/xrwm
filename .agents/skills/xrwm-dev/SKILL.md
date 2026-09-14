---
name: xrwm-dev
description: >
  REQUIRED for developing, testing, and maintaining the xrwm River 0.4 Wayland
  window manager. Use whenever modifying Rust source code (src/*), editing
  Cargo.toml, managing protocols (protocols/*), running quality checks
  (mise run check:plan, check:changed, fix, build), or following the strict
  Chronological Issue + Draft PR workflow (SOP).
---

# xrwm: Developer & Engineering Guide

This skill governs the engineering standards, quality gates, and development workflows for contributing to `xrwm`.

## Workflow (Issue + Draft PR)

All changes MUST follow the strict chronological lifecycle:

1. `gh issue view <id>` -> `git checkout -b <type>/issue-<id>-<name>` -> empty commit -> push -> `gh pr create --draft` (all tasks unchecked `- [ ]`).
2. Single-Item Loop: Implement only one unchecked item -> run `mise run check:plan` and `mise run check:changed` -> local atomic commit -> `git push` -> tick that item in the PR body with `gh pr edit --body-file` (preserving the rest of the body). Keep the PR in **draft**.
3. Finalize (only after every item is ticked and pushed): `gh pr ready` (awakens review bots once on the complete diff: CodeRabbit & Greptile). Never trigger review bots mid-round: CodeRabbit reviews incrementally and will not re-review earlier commits.
4. Review-Fix Loop: Ingest CodeRabbit `Prompt for AI Agents` and Greptile alerts (all reported findings & architectural warnings) -> defensively verify & commit fixes -> verify `gh pr checks` -> squash merge.

See [`references/issue-pr-workflow.md`](references/issue-pr-workflow.md) for full SOP.

## Quality Gates

```bash
mise run check:plan     # preview linter execution plan
mise run check:changed  # run rustfmt, clippy, prettier on modified files
mise run fix            # auto-format with hk
mise run build          # cargo build
mise run test           # cargo test
```
