# xrwm — River 0.4+ Window Manager

<p align="center">
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024_Edition-black?style=flat-square&logo=rust&logoColor=white" alt="Rust" /></a>
  <a href="https://wayland.freedesktop.org"><img src="https://img.shields.io/badge/Wayland-River_0.4+-005A9C?style=flat-square&logo=wayland&logoColor=white" alt="Wayland" /></a>
  <a href="https://kernel.org"><img src="https://img.shields.io/badge/Linux-Platform-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux" /></a>
  <a href="https://aur.archlinux.org/packages/xrwm-bin"><img src="https://img.shields.io/badge/Arch_Linux-AUR_Package-1793D1?style=flat-square&logo=archlinux&logoColor=white" alt="Arch Linux" /></a>
  <a href="https://github.com/xifan2333/xrwm/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/xifan2333/xrwm/ci.yml?branch=main&label=CI&style=flat-square&logo=githubactions&logoColor=white" alt="CI Status" /></a>
  <a href="https://www.gnu.org/licenses/gpl-3.0"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="License" /></a>
</p>

<p align="center">
  <b>English</b> | <a href="README_CN.md">简体中文</a>
</p>

---

`xrwm` is an ultra-lightweight, memory-safe, dynamic tiling Wayland window manager built for **[river](https://codeberg.org/river/river) 0.4+**, written in Rust.

It bridges the modern architectural separation of River 0.4 with the beloved **32-bit tag bitmask**, **dynamic master-stack tiling**, and **composable shell CLI configuration** of `river-classic`, while eliminating historical limitations with adjustable secondary stack ratios, smooth animations, and zero-alias canonical commands.

---

## 1. Core Principles & Highlights

- **River 0.4 Architectural Separation**:
  Operates as a pure Wayland client implementing `river-window-management-v1`, `river-layer-shell-v1`, and `river-xkb-bindings-v1`. If `xrwm` is restarted or crashes, your client applications and terminal sessions never terminate.
- **`river-classic` Soul**:
  - **32-bit Tag Bitmask**: Multi-tag focus, multi-tag window assignment, scratchpad drawers on tag 32.
  - **Master-Stack Dynamic Tiling**: Full orientation support (`left`, `right`, `top`, `bottom`), monocle mode, and floating windows.
  - **Infinite Shell Composability**: Configure everything via simple `xrwm <command>` invocations in your `~/.config/xrwm/init` shell script.
- **Adjustable Secondary Stack (`stack-ratio`)**:
  Overcomes river-classic's rigid 50/50 stack height limitation. Supports real-time mouse vertical drag resizing on secondary stack windows with dual-directional `NsResize` (↕) cursors and keyboard adjustments (`xrwm stack-ratio +/-0.05`).
- **Multi-Monitor & Spatial Navigation**:
  Independent per-output layouts and clipping areas. Spatial directional focus and window movement across displays (`focus-output`, `send-to-output`).
- **Smooth Animation Engine**:
  Buttery-smooth workspace slide transitions with cubic ease-out curves (`ease_out_cubic`) and boundary clipping (`calculate_clip_box`), plus smooth geometry transitions on window layout changes.
- **Strict Protocol Compliance & Single-Entry Dispatch**:
  All window management state modifications strictly adhere to River's `manage_start` sequence. Keyboard bindings, mouse bindings, and CLI calls share a single unified dispatch path.
- **Suckless Frugality**:
  Standalone stripped release binary (~1.2 MB), zero runtime dependencies, zero background garbage collection, and native streaming Waybar JSON support (`xrwm status --format waybar --stream`).

---

## 2. Project Layout

```text
xrwm/
├── Cargo.toml               # Package spec with size-optimized release profile
├── Makefile                 # Standard FHS installation rules
├── mise.toml                # Developer tools & quality tasks (hk, cargo, linters)
├── hk.pkl                   # Quality gates (rustfmt, clippy, taplo, prettier)
├── protocols/               # Wayland & River protocol XML definitions
├── doc/
│   ├── xrwm.1.md            # Official man page Markdown source
│   └── xrwm.1               # Compiled POSIX roff manual page
├── examples/
│   ├── init                 # Canonical starter configuration script
│   └── xrwm.desktop         # Standard Wayland session desktop entry
└── src/
    ├── main.rs              # Single-threaded event loop (poll), CLI client & daemon
    ├── protocol.rs          # Protocol client bindings generated via wayland-scanner
    ├── state.rs             # Application state machine (outputs, seats, tags, views)
    ├── layout.rs            # Master-stack tiling engine (view-padding & outer-padding)
    ├── tag.rs               # 32-bit bitmask tag engine (river-classic style)
    ├── animation.rs         # Cubic ease-out interpolation and clip-box calculations
    └── ipc.rs               # Command dispatcher & Waybar JSON status stream
```

---

## 3. Installation

### Arch Linux (AUR)

```bash
paru -S xrwm-bin
# or
yay -S xrwm-bin
```

### From Source (Makefile)

```bash
git clone https://github.com/xifan2333/xrwm.git
cd xrwm

# System-wide installation (installs xrwm, man xrwm, and desktop session)
sudo make install

# User-local installation (no root needed)
make install PREFIX=$HOME/.local
```

Once installed, **`man xrwm`** is immediately available anywhere in your terminal!

---

## 4. Quick Start

### 1. Launch xrwm with River

Start river with `xrwm` as its window manager:

```bash
river -c xrwm
```

### 2. Configuration (`~/.config/xrwm/init`)

Copy the example configuration into your config directory:

```bash
mkdir -p ~/.config/xrwm
cp examples/init ~/.config/xrwm/init
chmod +x ~/.config/xrwm/init
```

Sample configuration excerpt:

```bash
#!/usr/bin/env bash
export PATH="$HOME/.local/bin:$PATH"

# 1. Layout & Spacing (rivertile canonical syntax)
xrwm view-padding 8
xrwm outer-padding 4
xrwm main-ratio 0.55
xrwm stack-ratio 0.50
xrwm main-count 1
xrwm main-location left
xrwm default-attach-mode top

# 2. Decorations & Policies (riverctl canonical syntax)
xrwm border-width 2
xrwm border-color-focused '#61afef'
xrwm border-color-unfocused '#4b5263'
xrwm border-color-urgent '#e06c75'
xrwm focus-follows-cursor normal
xrwm set-cursor-warp on-output-change
xrwm animation true
xrwm animation-duration 150

# 3. Actions & Keybindings
xrwm map normal Super Return spawn foot
xrwm map normal Super W close
xrwm map normal Super P toggle-float
xrwm map normal Super F toggle-fullscreen
xrwm map normal "Super+Shift" Return zoom

# 4. Tags 1 to 9 (river-classic bitmask loop)
for i in $(seq 1 9); do
    tags=$((1 << (i - 1)))
    xrwm map normal Super "$i" set-focused-tags "$tags"
    xrwm map normal "Super+Shift" "$i" set-view-tags "$tags"
done

# 5. Pointer bindings
xrwm map-pointer normal Super BTN_LEFT move-view
xrwm map-pointer normal Super BTN_RIGHT resize-view
```

Reload changes at runtime anytime with:

```bash
xrwm reload
```

---

## 5. Manual & Documentation

Comprehensive documentation for all 49 commands, arguments, and options is available via the manual:

```bash
man xrwm
```

You can also read the online Markdown source in [`doc/xrwm.1.md`](doc/xrwm.1.md).

---

## 6. Developer Workflow & Quality Gates

This repository strictly adheres to the **Issue + Draft PR** lifecycle with automated quality gates:

```bash
mise run check:plan     # preview linter execution plan
mise run check:changed  # run rustfmt, clippy, taplo on modified files
mise run fix            # auto-format modified files
mise run build          # compile debug binary
mise run build:release  # compile optimized release binary
mise run test           # run test suite
mise run doc            # recompile man page from Markdown via pandoc
```

---

## 7. License

GPL-3.0-only
