# Agent Instructions & Project Guidelines for xrwm

`xrwm` is a River 0.4+ Wayland window manager written in Rust, combining the 32-bit tag bitmask and dynamic tiling of `river-classic` with composable shell CLI configuration.

---

## 1. Project Structure

| Goal                            | Target File / Directory     |
| ------------------------------- | --------------------------- |
| Package specs & dependencies    | `Cargo.toml`                |
| Developer tooling & tasks       | `mise.toml`                 |
| Quality gates & git hooks       | `hk.pkl`                    |
| Wayland & River XML protocols   | `protocols/`                |
| Application entry & IPC         | `src/main.rs`, `src/ipc.rs` |
| State machine & window tracking | `src/state.rs`              |
| Dynamic tiling layout engine    | `src/layout/`               |
| 32-bit tag bitmask engine       | `src/tag.rs`                |

---

## 2. Code Quality & Quality Gates

This repository uses **hk** (`hk.pkl`) for git hooks and code quality checks:

- **Rust formatting**: `rustfmt` (via `Builtins.rustfmt` or `cargo fmt`)
- **Rust linting**: `cargo clippy --all-targets -- -D warnings`
- **TOML**: `taplo` (with `--no-schema`)
- **Shell / Markdown**: `shellcheck`, `shfmt`, `prettier`

Run scoped checks during development:

```bash
mise run check:plan     # preview which linters will run
mise run check:changed  # run checks on modified/untracked files
mise run fix            # auto-format modified files
mise run build          # compile project
```

---

## 3. Strict Chronological Development Workflow (Issue + Draft PR)

All changes must follow the SOP documented in `.agents/skills/xrwm-dev/references/issue-pr-workflow.md`:

1. `gh issue view <id>` -> checkout branch -> empty commit -> push -> `gh pr create --draft` (all `- [ ]`).
2. Single-Item Focused Development -> Local Quality Gate (`mise run check:plan`, `mise run check:changed`) -> Local Atomic Commit.
3. Unified Push, Checks & Merge (`git push`, `gh pr edit`, `gh pr ready`, `gh pr merge --squash --delete-branch`).
