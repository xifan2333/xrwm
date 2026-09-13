---
title: XRWM
section: 1
header: xrwm Manual
footer: xrwm 0.1.0
date: September 2026
---

# NAME

xrwm - River 0.4+ dynamic tiling Wayland window manager

# SYNOPSIS

**xrwm**\
**xrwm** _command_ [_arguments..._]

# DESCRIPTION

**xrwm** is an ultra-lightweight, memory-safe Wayland window manager built for
**river(1)** 0.4+, written in Rust.

It combines the architectural process separation of River 0.4 with the beloved
32-bit tag bitmask, dynamic master-stack tiling, and composable shell-script
configuration of river-classic.

When run without arguments, **xrwm** acts as the window manager daemon and connects
to the running **river(1)** compositor. When run with arguments, it acts as a
client controller communicating with the daemon over a UNIX domain socket.

# TERMINOLOGY

_view_
: A managed application window (e.g. an xdg-toplevel).

_output_
: A logical screen or monitor display area.

_tag_
: A 32-bit bitmask identifying workspace assignment (from 1 to 32). A view may
belong to multiple tags, and multiple tags may be focused simultaneously.

_master_
: The primary column or row in the tiling layout.

_stack_
: The secondary area of windows arranged beside the master area.

# COMMANDS

## WINDOW ACTIONS

**close**
: Closes the currently focused window gracefully.

**zoom**
: Promotes the focused window to the master position in the layout stack.
If the master window is already focused, promotes the second view in the stack to
master (toggles between the two most active windows).

**toggle-float**
: Toggles floating or tiled state on the focused window. When returning to floating,
preserves its previous floating geometry.

**toggle-fullscreen**
: Toggles fullscreen state on the focused window.

**focus-view** [**-skip-floating**] _direction_
: Shifts window focus in the specified direction. Supported directions are
**next**, **previous** (or **prev**), **left**, **right**, **up**, and **down**.
If **-skip-floating** is passed, floating windows are ignored during navigation.

**swap** _direction_
: Swaps the focused window with the window in the specified direction (**next**,
**previous**, **left**, **right**, **up**, **down**). Allows reordering within the
secondary stack without disturbing the master view.

**focus-output** _direction_
: Shifts focus across displays in the specified logical or spatial direction
(**next**, **previous**, **left**, **right**, **up**, **down**).

**send-to-output** [**-current-tags**] _direction_
: Sends the focused window to the target output in the specified direction.
If **-current-tags** is passed, automatically updates the window's tag mask to
match the active tags of the destination output.

**resize** **horizontal**|**vertical** _delta_
: Resizes floating windows by _delta_ pixels, or adjusts tiled layout proportions
(**horizontal** adjusts **main-ratio**; **vertical** adjusts **stack-ratio**).

**move** **left**|**right**|**up**|**down** _delta_
: Translates a floating window by _delta_ logical pixels. Ignored on tiled windows.

**snap** **left**|**right**|**up**|**down**
: Converts the focused window to floating and snaps it to cover the corresponding
half of the screen (50% width on left/right, or 50% height on up/down).

**exit**
: Terminates the **xrwm** window manager daemon.

**reload**
: Re-executes the user configuration script (`~/.config/xrwm/init`).

**ping**
: Health check request, returning `pong`.

## TAG MANAGEMENT

**set-focused-tags** _tags_
: Sets the visible tag bitmask on the current output.

**set-view-tags** _tags_
: Sets the tag bitmask assignment of the currently focused window.

**toggle-focused-tags** _tags_
: Toggles the visibility of the specified tag bits on the current output.
Enables viewing multiple tags simultaneously or toggling scratchpads (tag 32).

**toggle-view-tags** _tags_
: Toggles the specified tag bits on the currently focused window.

**focus-previous-tags**
: Jumps back and forth between the current and previous tag configuration.

**send-to-previous-tags**
: Sends the focused window to the previous tag configuration.

**spawn-tagmask** _tags_
: Sets a bitmask restricting the initial tag assignment of newly created views.
New windows receive `focused_tags & spawn_tagmask`. Defaults to `0xFFFFFFFF`.

## LAYOUT CONFIGURATION

**view-padding** _pixels_
: Sets the inner padding between windows in pixels. Defaults to `4`.

**outer-padding** _pixels_
: Sets the outer padding between the window layout perimeter and the screen edge in pixels. Defaults to `4`.

**main-location** **left**|**right**|**top**|**bottom**
: Sets the placement orientation of the master area. Defaults to **left**.

**main-count** _count_
: Sets the number of windows allocated to the master area. Can be absolute (e.g. `2`)
or relative (e.g. `+1`, `-1`). Defaults to `1`.

**main-ratio** _ratio_
: Sets the master area proportion relative to the layout area, clamped to `0.1`..`0.9`.
Can be absolute (e.g. `0.60`) or relative (e.g. `+0.05`, `-0.05`). Defaults to `0.55`.

**stack-ratio** _ratio_
: Sets the secondary stack height division proportion, clamped to `0.1`..`0.9`.
Can be absolute (e.g. `0.65`) or relative (e.g. `+0.05`, `-0.05`). Defaults to `0.50`.

**default-attach-mode** **top**|**bottom**|**above**|**below**|**after** _N_
: Sets the insertion position for newly created windows in the stack:

- **top**: Inserts at the top of the stack (becomes master).
- **bottom**: Appends to the end of the stack.
- **above**: Inserts above the currently focused window.
- **below**: Inserts below the currently focused window.
- **after** _N_: Inserts after _N_ windows in the stack.

## APPEARANCE & POLICIES

**border-width** _pixels_
: Sets server-side decoration (SSD) border thickness in pixels. Defaults to `2`.

**border-color-focused** _color_
: Sets the border color of the focused window (hex format `#RRGGBB` or `0xRRGGBB`).

**border-color-unfocused** _color_
: Sets the border color of unfocused windows.

**border-color-urgent** _color_
: Sets the border color of windows demanding urgency attention.

**set-cursor-warp** **disabled**|**on-output-change**|**on-focus-change**
: Configures cursor warp policy:

- **disabled**: Cursor is never automatically warped.
- **on-output-change**: Cursor automatically warps to the center of the destination screen on output focus change.
- **on-focus-change**: Cursor warps to the center of the newly focused window or screen.

**focus-follows-cursor** **disabled**|**normal**|**always**
: Configures pointer hover focus policy:

- **normal**: Automatically focuses views when the pointer crosses over borders.
- **disabled**: Pointer movement does not alter keyboard focus; only clicks or key bindings change focus.
- **always**: The view under the pointer is always focused on any movement.

**hide-cursor** **timeout** _milliseconds_
: Automatically hides the cursor after _milliseconds_ of inactivity. Set to `0` to disable.

**hide-cursor** **when-typing** **enabled**|**disabled**
: Automatically hides the cursor when typing on the keyboard. Moving the pointer immediately restores visibility.

**animation** **true**|**false**
: Enables or disables window geometry and workspace slide transition animations. Defaults to `true`.

**animation-duration** _milliseconds_
: Sets the cubic ease-out animation transition duration in milliseconds. Defaults to `150`.

## MAPPINGS & MODES

**declare-mode** _mode_
: Declares a new custom modal keybinding mode.

**enter-mode** _mode_
: Enters the specified keybinding mode (built-in modes include **normal** and **locked**).

**map** _mode_ _modifiers_ _key_ _action..._
: Maps a keyboard key combination in _mode_ to execute an action. Modifiers include
`Super` (or `Mod4`), `Shift`, `Ctrl`, `Alt` (or `Mod1`), `None`.

**map-pointer** _mode_ _modifiers_ _button_ _action..._
: Maps a mouse button press in _mode_ to an action (e.g. `BTN_LEFT move-view`, `BTN_RIGHT resize-view`).

**unmap** _mode_ _modifiers_ _key_
: Dynamically unregisters a key binding in _mode_.

**unmap-pointer** _mode_ _modifiers_ _button_
: Dynamically unregisters a mouse pointer binding in _mode_.

## WINDOW RULES

**rule-add** [**-app-id** _glob_] [**-title** _glob_] _action_ [_arguments..._]
: Adds a window matching rule. _glob_ supports standard wildcards (`*`). Available actions:

- **float** / **no-float**: Force floating or tiled mode.
- **ssd** / **csd**: Force server-side borders or client-side decorations.
- **tags** _mask_: Assign initial tag bitmask.
- **dimensions** _width_ _height_: Set initial floating width and height (centered).
- **position** _x_ _y_: Set initial floating window position.
- **fullscreen**: Automatically launch window in fullscreen mode.
- **output** _name|id_: Assign initial target display.

**rule-del** [**-app-id** _glob_] [**-title** _glob_] _action_
: Deletes a previously added window rule matching the criteria.

**list-rules** [_action_]
: Prints all active window rules, optionally filtered by action type.

## STATUS & QUERY

**status** [**--format** **waybar**] [**--stream**]
: Queries window manager state. Without flags, outputs a single JSON object with
tags, window list, geometry, and layout details. With **--format waybar**, outputs
formatted JSON for Waybar custom modules. With **--stream**, maintains a persistent
pipe streaming JSON updates on every state change.

# FILES

`$XDG_CONFIG_HOME/xrwm/init` or `~/.config/xrwm/init`
: User configuration script executed upon daemon startup and reloaded via `xrwm reload`.

`$XDG_RUNTIME_DIR/xrwm-$WAYLAND_DISPLAY.sock`
: UNIX domain socket used for IPC communication.

# AUTHORS

Developed by xifan2333 and contributors.

# SEE ALSO

**river(1)**, **waybar(1)**, **foot(1)**
