# xrwm — River 0.4+ Window Manager

`xrwm` is an ultra-lightweight, memory-safe Wayland window manager built for **[river](https://codeberg.org/river/river) 0.4+**, written in Rust.

It bridges the architectural separation of River 0.4 with the beloved **32-bit tag bitmask**, **dynamic master-stack tiling**, and **composable shell-script configuration** of `river-classic`.

---

## 1. Core Principles

- **River 0.4 Protocol Separation**: Pure client implementing `river-window-management-v1`, `river-layer-shell-v1`, and `river-xkb-bindings-v1`. If `xrwm` restarts, client applications never crash.
- **`river-classic` Soul**:
  - 32-bit tag bitmask system: multi-tag focus, multi-tag window assignment, scratchpad tag 32.
  - Classic dynamic master-stack tiling (rivertile style) + monocle + floating.
  - Infinite composability through shell-script CLI commands (`xrwm-ctl`).
- **Suckless Frugality**:
  - Standalone, stripped binary (< 1 MB).
  - Native Waybar JSON status stream (`xrwm --waybar`).
  - Zero runtime dependencies, zero background garbage collection.

---

## 2. Project Layout

```text
xrwm/
├── Cargo.toml               # Package spec with size-optimized release profile
├── mise.toml                # Developer tools & quality tasks (hk, cargo, linters)
├── hk.pkl                   # Pre-commit quality gates (rustfmt, clippy, taplo)
├── protocols/               # Wayland & River protocol XML definitions
└── src/
    ├── main.rs              # CLI entry point, IPC socket listener, event loop
    ├── protocol.rs          # Protocol client bindings generated via wayland-scanner
    ├── state.rs             # Application state machine (outputs, seats, tags, views)
    ├── layout/              # Dynamic tiling engine (master-stack, monocle, float)
    ├── tag.rs               # 32-bit bitmask tag engine (river-classic style)
    ├── binds.rs             # Keybinding and input mapping registration
    └── ipc.rs               # Command dispatcher & Waybar JSON status stream
```

---

## 3. Developer Workflow

This repository strictly adheres to the **Issue + Draft PR** development lifecycle:

```bash
mise run hooks          # install git pre-commit hooks
mise run check:plan     # preview linter execution plan
mise run check:changed  # run rustfmt, clippy, taplo on modified files
mise run fix            # auto-format with hk
mise run build          # cargo build
mise run test           # cargo test
```

---

## 4. License

GPL-3.0-only
