# xrwm Domain Glossary

The domain language and core invariants of xrwm, a River 0.4+ dynamic tiling Wayland window manager combining 32-bit tag bitmasks with composable CLI configuration.

## Core Entities

**Window**:
A managed top-level application surface (Wayland xdg-toplevel) tracked by xrwm.
_Avoid_: Surface, client, view (external user-facing CLI/man term only)

**View**:
The user-facing CLI and IPC name for a managed Window.
_Avoid_: Window in user documentation

**Output**:
A physical or virtual display monitor providing a bounded rendering area and layout boundary.
_Avoid_: Screen, monitor, display

**Seat**:
A Wayland input group encompassing keyboard, pointer focus, and active interactive gestures.
_Avoid_: Device, user, cursor group

**Tag**:
A 1-indexed workspace bitmask channel (bits 1 through 32) determining window visibility per output.
_Avoid_: Workspace, desktop, group

## Geometry & Layout Invariants

**Tiled Geometry**:
The calculated target position and dimensions assigned to a non-floating window by the layout engine.
_Avoid_: Normal layout, grid position

**Floating Geometry**:
The user- or rule-configured resting box of an unconstrained window, preserved across tiled switches and output migrations.
_Avoid_: Saved size, unconstrained rect

**Floating Invariant**:
The rule that a floating window's geometry and rule dimensions cannot be overwritten by intermediate unconfigured client buffer commits.
_Avoid_: Geometry freeze, sticky size

**Dimension Proposal**:
An asynchronous request sent to a Wayland client via the River protocol proposing target buffer dimensions.
_Avoid_: Resize event, configure request

**Dimension Proposal Invariant**:
The contract that proposals are emitted monotonically and only when dimensions actually change or are unconfigured, preventing Wayland frame deadlocks.
_Avoid_: Proposal throttle, dimension lock

## Rendering & Frame Lifecycle

**Frame Plan**:
A pure, side-effect-free description of all window transformations, clipping rectangles, borders, and Z-orders to be applied during a compositor render sequence.
_Avoid_: Render state, draw command list, display list

**Render Frame**:
The concrete visual rectangle and clipping state assigned to a window for the current display refresh tick.
_Avoid_: Draw frame, buffer geometry

## Tag & Workspace Dispatching

**Tag Dispatcher**:
The centralized resolution authority on the application state that determines active workspace tag states and window visibility across physical outputs and headless fallbacks.
_Avoid_: Tag manager, workspace router

**Floating Collapse**:
The per-output visibility state toggled via `toggle-floating-views` that collapses unconstrained windows offscreen while preserving geometry and moving focus to a visible tiled window.
_Avoid_: Minimize all, float hide
