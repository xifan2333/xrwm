# xrwm — River 0.4+ 动态平铺窗口管理器

<p align="center">
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024_Edition-black?style=flat-square&logo=rust&logoColor=white" alt="Rust" /></a>
  <a href="https://wayland.freedesktop.org"><img src="https://img.shields.io/badge/Wayland-River_0.4+-005A9C?style=flat-square&logo=wayland&logoColor=white" alt="Wayland" /></a>
  <a href="https://kernel.org"><img src="https://img.shields.io/badge/Linux-Platform-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux" /></a>
  <a href="https://aur.archlinux.org/packages/xrwm-bin"><img src="https://img.shields.io/badge/Arch_Linux-AUR_Package-1793D1?style=flat-square&logo=archlinux&logoColor=white" alt="Arch Linux" /></a>
  <a href="https://github.com/xifan2333/xrwm/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/xifan2333/xrwm/ci.yml?branch=main&label=CI&style=flat-square&logo=githubactions&logoColor=white" alt="CI Status" /></a>
  <a href="https://www.gnu.org/licenses/gpl-3.0"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="License" /></a>
</p>

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

---

`xrwm` 是一款专为 **[river](https://codeberg.org/river/river) 0.4+** 打造的极简、内存安全、基于 Rust 编写的 Wayland 动态平铺窗口管理器。

它完美继承了 River 0.4 现代化架构的进程解耦特性，兼具 `river-classic` 备受推崇的 **32 位 Tag 位掩码系统**、**动态主从排版（Master-Stack）** 与 **极速可组合 Shell 脚本配置**；同时克服了原版从栏高度不可调的痛点，引入动态副栏比例调整、平滑动画引擎与零别名规范化命令体系。

---

## 1. 核心设计原则与技术亮点

- **River 0.4 协议分离架构**：
  作为纯粹的 Wayland 客户端实现 `river-window-management-v1`、`river-layer-shell-v1` 与 `river-xkb-bindings-v1`。即使 `xrwm` 发生重启或重载，你的终端会话与客户端程序永远不会退出或崩溃。
- **`river-classic` 经典精髓**：
  - **32-bit Tag 位掩码系统**：支持多工作区并存复选、多标签窗口归属，以及 Tag 32 便签抽屉（Scratchpad）。
  - **Master-Stack 动态主从排版**：支持全方位分栏（`left`、`right`、`top`、`bottom`）、单窗口全屏 Monocle 模式及悬浮窗口。
  - **无限组合的 Shell 配置体验**：在 `~/.config/xrwm/init` 中通过简单的 `xrwm <command>` 自由编写脚本组合，无需学习复杂的专有 DSL。
- **副栏高度动态调节突破（`stack-ratio`）**：
  彻底克服 river-classic 固化等分从栏高度的顽疾。支持鼠标在右侧副栏窗口直接按住 `Super + RMB` 垂直拖拽实时调整高度分割比，附带专属 `NsResize`（↕）光标；键盘端亦支持 `xrwm stack-ratio +/-0.05` 即时步进调整。
- **多显示器独立排版与空间流转**：
  为每个显示器维护独立的物理几何、分辨率及 Layer-shell 可用区域。支持 2D 真实空间方位切屏与送窗（`focus-output`、`send-to-output`）。
- **现代平滑动画引擎**：
  跨 Tag 工作区切换横向滑动动画（三次缓动 `ease_out_cubic` + 精准边界裁剪 `calculate_clip_box`）；新开窗口展开动画与窗口尺寸排版平滑缩放过渡，带来媲美 Hyprland 的高级桌面质感。
- **严格时序规范与单一分发入口**：
  所有窗口管理状态修改严格遵循 River 0.4 `manage_start` 协议序列。键盘映射、鼠标映射与 CLI 命令行调用共享统一分发源，杜绝行为偏差。
- **极简克制与极低开销（Suckless Frugality）**：
  Release 静态二进制仅 ~1.2 MB，零外部运行时依赖，零后台垃圾回收开销，并原生内建长连接 Waybar JSON 状态流推送（`xrwm status --format waybar --stream`）。

---

## 2. 目录架构

```text
xrwm/
├── Cargo.toml               # 包规范与体积优化编译配置
├── Makefile                 # 标准 Unix FHS 系统安装规则
├── mise.toml                # 开发者工具与质量门禁任务
├── hk.pkl                   # Git pre-commit 钩子规范 (rustfmt, clippy, taplo, prettier)
├── protocols/               # Wayland & River XML 协议定义
├── doc/
│   ├── xrwm.1.md            # 官方 UNIX Man 手册 Markdown 源码
│   └── xrwm.1               # 编译生成的标准 POSIX roff 格式手册
├── examples/
│   ├── init                 # 规范化标准初始化范例脚本
│   └── xrwm.desktop         # 标准 Wayland 会话入口文件 (适配 SDDM/GDM/greetd)
└── src/
    ├── main.rs              # 单线程事件循环 (poll)、CLI 客户端与 Daemon 守护进程
    ├── protocol.rs          # 经 wayland-scanner 生成的协议客户端绑定
    ├── state.rs             # 窗口管理器状态机 (outputs, seats, tags, views, 动画时钟)
    ├── layout.rs            # 动态主从排版引擎 (view-padding 与 outer-padding 双层间距)
    ├── tag.rs               # 32 位位掩码标签计算引擎 (river-classic 风格)
    ├── animation.rs         # 三次缓动曲线、几何插值与裁剪框数学计算
    └── ipc.rs               # 命令解析器与 Waybar JSON 状态广播流
```

---

## 3. 安装指引

### Arch Linux (AUR)

```bash
paru -S xrwm-bin
# 或
yay -S xrwm-bin
```

### 源码编译安装 (Makefile)

```bash
git clone https://github.com/xifan2333/xrwm.git
cd xrwm

# 系统全局安装（一键安装二进制、man 手册与桌面会话）
sudo make install

# 用户家目录本地安装（无需 root 权限）
make install PREFIX=$HOME/.local
```

安装完成后，在终端任何路径直接输入 **`man xrwm`** 即可查阅完整官方手册！

---

## 4. 快速上手

### 1. 配合 River 启动

在 River 启动命令行中将 `xrwm` 作为窗口管理器传入：

```bash
river -c xrwm
```

### 2. 用户配置脚本 (`~/.config/xrwm/init`)

复制范例配置并赋予可执行权限：

```bash
mkdir -p ~/.config/xrwm
cp examples/init ~/.config/xrwm/init
chmod +x ~/.config/xrwm/init
```

配置脚本精选范例：

```bash
#!/usr/bin/env bash
export PATH="$HOME/.local/bin:$PATH"

# 1. 布局与间隙 (rivertile 标准语法)
xrwm view-padding 8
xrwm outer-padding 4
xrwm main-ratio 0.55
xrwm stack-ratio 0.50
xrwm main-count 1
xrwm main-location left
xrwm default-attach-mode top

# 2. 边框与策略 (riverctl 标准语法)
xrwm border-width 2
xrwm border-color-focused '#61afef'
xrwm border-color-unfocused '#4b5263'
xrwm border-color-urgent '#e06c75'
xrwm focus-follows-cursor normal
xrwm set-cursor-warp on-output-change
xrwm animation true
xrwm animation-duration 150

# 3. 窗口动作与快捷键映射
xrwm map normal Super Return spawn foot
xrwm map normal Super W close
xrwm map normal Super P toggle-float
xrwm map normal Super F toggle-fullscreen
xrwm map normal "Super+Shift" Return zoom

# 4. 工作区 Tag 1 到 9 (标准 riverctl 位运算循环)
for i in $(seq 1 9); do
    tags=$((1 << (i - 1)))
    xrwm map normal Super "$i" set-focused-tags "$tags"
    xrwm map normal "Super+Shift" "$i" set-view-tags "$tags"
done

# 5. 鼠标操作绑定
xrwm map-pointer normal Super BTN_LEFT move-view
xrwm map-pointer normal Super BTN_RIGHT resize-view
```

在终端中执行以下命令即可即时热重载配置：

```bash
xrwm reload
```

---

## 5. 完整命令与手册

所有 49 条命令、参数取值与默认值均可通过系统手册详尽查阅：

```bash
man xrwm
```

亦可在网页端查阅源码版本：[`doc/xrwm.1.md`](doc/xrwm.1.md)。

---

## 6. 开发者工作流与质量门禁

本仓库严格践行 **Issue + Draft PR** 的开发闭环，并通过全量代码门禁保护质量：

```bash
mise run check:plan     # 预览静态检查执行计划
mise run check:changed  # 针对变更文件执行 rustfmt、clippy 与 taplo 检查
mise run fix            # 自动修复格式化问题
mise run build          # 编译 Debug 二进制
mise run build:release  # 编译优化版 Release 二进制
mise run test           # 运行全量单元测试套件
mise run doc            # 通过 pandoc 从 Markdown 动态重新编译 man 手册
```

---

## 7. 许可证

GPL-3.0-only
