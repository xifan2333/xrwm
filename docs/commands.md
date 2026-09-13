# xrwm 命令参考与规范手册 (Command Reference & Architecture Spec)

`xrwm` 是专为 River 0.4+ 设计的轻量、内存安全平铺式窗口管理器，兼具 `river-classic` 的 32 位 Tag 位掩码与动态主从排版灵魂，并将原版分立的 `riverctl` 与 `rivertile` 统一为单一同构 CLI 工具。

---

## 1. 运行范式

```bash
# 窗口管理器守护进程启动（在 river 启动配置中调用）
river -c xrwm

# 客户端控制调用（在终端、脚本、快捷键或 Waybar 中调用，退出码 0 为成功，1 为错误）
xrwm <command> [arguments...]
```

---

## 2. 现役命令全量清单 (Active Commands)

```bash
# ==============================================================================
# 1. 窗口管理与动作 (Window Management & Actions)
# ==============================================================================

# 优雅关闭当前聚焦窗口
xrwm close

# 将当前聚焦窗口提升为主窗口（Master 顶层）；若当前已在主栏，则提升副栏顶层窗口（双向 Toggle 对切）
xrwm zoom

# 切换当前聚焦窗口的 悬浮/平铺 状态
xrwm toggle-float

# 切换当前聚焦窗口的全屏状态
xrwm toggle-fullscreen

# 沿逻辑顺序或 2D 空间方向流转窗口焦点（可选 -skip-floating 忽略悬浮窗口）
# 方向参数支持：next | previous | left | right | up | down
xrwm focus-view next
xrwm focus-view left
xrwm focus-view -skip-floating next

# 将当前聚焦窗口与目标方向上的窗口物理位置对调
# 方向参数支持：next | previous | left | right | up | down
xrwm swap next
xrwm swap right

# 流转多显示器焦点，支持空间方位判定（left/right/up/down）与逻辑循环（next/prev）
xrwm focus-output next
xrwm focus-output right

# 将当前聚焦窗口发送到目标显示器，并自动接入目标显示器的独立布局栈
# 可选 -current-tags 参数自动将窗口 tags 同步为目标显示器的当前激活 tags
xrwm send-to-output next
xrwm send-to-output -current-tags right

# 将当前聚焦窗口转换为浮动窗，并精准贴附到屏幕边缘半屏（Windows Snap 经典手感）
# 支持：left（左半屏）| right（右半屏）| up（上半屏）| down（下半屏）
xrwm snap left
xrwm snap right
xrwm snap up
xrwm snap down

# 键盘平移浮动窗口（像素）：
# 方向参数支持：left | right | up | down
xrwm move left 50
xrwm move right 50
xrwm move up 50
xrwm move down 50

# 调整窗口尺寸：
# - 悬浮窗口：调整宽度/高度像素（+/-delta）
# - 平铺窗口：horizontal 调节主栏比例（main-ratio），vertical 调节副栏高度比例（stack-ratio）
xrwm resize horizontal 20
xrwm resize vertical -20

# 守护进程健康检查，成功返回 "pong"
xrwm ping

# 优雅终止 xrwm 窗口管理器守护进程
xrwm exit

# 重新执行并热加载 ~/.config/xrwm/init 配置脚本
xrwm reload


# ==============================================================================
# 2. 标签与工作区管理 (Tag Management)
# ==============================================================================

# 设置当前屏幕聚焦的 32 位 Tag 位掩码 (如 1=Tag1, 2=Tag2, 3=Tag1+2, 511=Tag1..9)
xrwm set-focused-tags 3

# 设置当前聚焦窗口归属的 32 位 Tag 位掩码
xrwm set-view-tags 3

# 翻转指定 Tag 掩码的可见性（实现多标签并存，或结合 Tag 32 实现 Scratchpad 抽屉）
xrwm toggle-focused-tags 2147483648

# 翻转当前窗口的 Tag 掩码绑定
xrwm toggle-view-tags 4

# 快速往返跳转到上一次查看的标签组合 (Tab 极速对切)
xrwm focus-previous-tags

# 将当前窗口发送到上一次查看的标签组合
xrwm send-to-previous-tags

# 设置新窗口初始标签掩码过滤（按位与计算，防止新开窗口污染特化标签，默认 0xFFFFFFFF）
xrwm spawn-tagmask 511


# ==============================================================================
# 3. 排版与间隙引擎 (Layout Engine & Gaps - 100% 对齐 rivertile)
# ==============================================================================

# 设置主栏（Master）宽高分割比例，范围 0.1 到 0.9（默认 0.55）
# 支持绝对值或 +/-delta 相对调节
xrwm main-ratio 0.60
xrwm main-ratio +0.05
xrwm main-ratio -0.05

# [xrwm 独创突破] 设置副栏（Stack）上下高度分割比例，范围 0.1 到 0.9（默认 0.50）
# 突破 river-classic 固化等分限制，支持绝对值或相对调节
xrwm stack-ratio 0.65
xrwm stack-ratio +0.05
xrwm stack-ratio -0.05

# 设置主栏（Master）容纳的窗口数量，最小为 1（默认 1）
# 支持绝对值或相对调节
xrwm main-count 2
xrwm main-count +1
xrwm main-count -1

# 设置主栏方位（主从排版方向）
# 支持：left（主栏在左）| right（主栏在右）| top（主栏在上）| bottom（主栏在下）
xrwm main-location left
xrwm main-location top

# 设置窗口间隙内外边距（像素，默认 4px，100% 对齐 rivertile 原名）
xrwm view-padding 8

# 设置新创建窗口的排版插入策略（默认 top，100% 对齐 riverctl 原名）
# - top: 插入为主窗口（Master 顶端）
# - bottom: 追加到副栏末尾（不打扰当前工作流）
# - above: 插入到当前聚焦窗口的上方/前面
# - below: 插入到当前聚焦窗口的下方/后面
# - after <N>: 插入到第 N 个窗口之后（如 after 1 始终插入为副栏第 1 个窗口）
xrwm default-attach-mode top
xrwm default-attach-mode bottom
xrwm default-attach-mode after 1


# ==============================================================================
# 4. 边框装饰与视觉风格 (Decorations & Styling - 100% 对齐 riverctl)
# ==============================================================================

# 设置服务端边框（SSD）宽度（像素，默认 2px）
xrwm border-width 2

# 设置当前聚焦窗口的边框颜色（支持 #RRGGBB 或 0xRRGGBB）
xrwm border-color-focused '#61afef'

# 设置未聚焦窗口的边框颜色
xrwm border-color-unfocused '#4b5263'

# 设置紧急提示（Urgent）窗口的边框颜色
xrwm border-color-urgent '#e06c75'

# 开启或关闭过渡动画引擎
# 支持：true | false | on | off | 1 | 0
xrwm animation true

# 设置动画缓动过渡时长（毫秒，默认 150ms）
xrwm animation-duration 150


# ==============================================================================
# 5. 鼠标光标与交互策略 (Cursor & Pointer Policy - 100% 对齐 riverctl)
# ==============================================================================

# 设置鼠标光标瞬移吸附策略（100% 对齐 riverctl 原名）
# - disabled: 关闭光标吸附（默认）
# - on-output-change: 切换显示器焦点时，光标自动瞬移到目标屏幕中心
# - on-focus-change: 无论是切换屏幕还是切换窗口，光标均瞬移到目标几何中心
xrwm set-cursor-warp on-output-change
xrwm set-cursor-warp disabled

# 设置鼠标焦点跟随策略（默认 normal）
# - normal: 鼠标指针跨越边框划入新窗口时自动聚焦
# - disabled: 纯键盘流防误触模式，鼠标滑过不抢焦点，点击窗口或快捷键才聚焦
# - always: 无论指针是否移动跨界，指针下的窗口始终聚焦
xrwm focus-follows-cursor normal
xrwm focus-follows-cursor disabled

# 绑定鼠标按键操作（按键名为标准 libinput 事件名，如 BTN_LEFT / BTN_RIGHT / BTN_MIDDLE）
xrwm map-pointer normal Super BTN_LEFT move-view
xrwm map-pointer normal Super BTN_RIGHT resize-view
xrwm map-pointer normal Super BTN_MIDDLE toggle-float


# ==============================================================================
# 6. 键盘映射与模态系统 (Keybindings & Modes - 100% 对齐 riverctl)
# ==============================================================================

# 声明自定义模式（默认已有 normal, locked）
xrwm declare-mode resize

# 切换进入指定模式
xrwm enter-mode resize
xrwm enter-mode normal

# 在指定模式下映射快捷键（原生 riverctl 格式：map <mode> <modifiers> <key> <action...>）
xrwm map normal Super Return spawn foot
xrwm map normal Super Q close
xrwm map normal "Super+Shift" Return zoom
xrwm map normal Super H focus-view left
xrwm map normal Super L focus-view right

# 动态注销指定模式下的按键映射
xrwm unmap normal Super Q

# 动态注销指定模式下的鼠标按键映射
xrwm unmap-pointer normal Super BTN_LEFT


# ==============================================================================
# 7. 窗口规则系统 (Window Rules - 100% 对齐 riverctl)
# ==============================================================================

# 基础语法：xrwm rule-add [-app-id <glob>] [-title <glob>] <action> [args...]
# 匹配器支持通配符 *，如 "floating-terminal*"、"Peek*"

# 浮动规则
xrwm rule-add -app-id "imv" float
xrwm rule-add -app-id "mpv" float
xrwm rule-add -app-id "wechat" float

# 装饰边框规则（强制 SSD 边框或 CSD 客户端自绘无边框）
xrwm rule-add -app-id "wofi" csd
xrwm rule-add -app-id "foot" ssd

# 固定工作区规则（指定初始启动到哪个 Tag 位掩码）
xrwm rule-add -app-id "zen-browser" tags 1
xrwm rule-add -app-id "wechat" tags 256

# 初始尺寸规则（指定浮动窗口的宽高，系统自动在屏幕正中央居中放置）
xrwm rule-add -app-id "mpv" dimensions 960 540
xrwm rule-add -app-id "imv" dimensions 960 540
xrwm rule-add -app-id "wofi" dimensions 800 600

# 初始坐标规则（指定浮动窗口弹出的精确 x y 坐标，覆盖默认居中）
xrwm rule-add -app-id "calculator" position 100 100

# 初始全屏规则（特定应用启动即以全屏模式运行）
xrwm rule-add -app-id "gamescope" fullscreen
xrwm rule-add -app-id "mpv" fullscreen

# 固定显示器规则（指定应用生成在特定屏幕上，支持 output ID 或 1..N 序号）
xrwm rule-add -app-id "wechat" output 1

# 列出当前生效的所有窗口规则列表（可选按 action 过滤，如 float/dimensions/tags）
xrwm list-rules
xrwm list-rules float

# 动态删除已配置的单条窗口规则
xrwm rule-del -app-id "mpv" float
xrwm rule-del -app-id "wofi" csd


# ==============================================================================
# 8. 状态查询与第三方工具集成 (Status & Integration)
# ==============================================================================

# 获取当前完整的状态 JSON（包含激活标签、窗口列表、几何坐标、聚焦ID、排版模式等）
xrwm status

# 获取适配 Waybar 自定义模块的单行 JSON 格式
xrwm status --format waybar

# 建立长连接管道，持续监听并流式推送每次窗口、标签或焦点变动的 JSON 状态流
xrwm status --stream
xrwm status --format waybar --stream
```
