# Architecture

Windows-only Rust workspace under `packages/`. `default-members =
["packages/wm", "packages/wm-cli", "packages/wm-watcher"]` — bare `cargo`
commands build everything. All paths below are relative to `packages/`.

## Crate map

| Crate | Kind | Responsibility |
|---|---|---|
| `wm` | bin `lonewm` | Core WM: event loop, container tree, commands, events, IPC server, tray |
| `wm-cli` | bin `lonewm-cli` | CLI → IPC passthrough; also relaunches `lonewm.exe` for `Start` (UIAccess exes can't be spawned directly) |
| `wm-common` | lib | Shared vocabulary: `ParsedConfig`, `AppCommand`/`InvokeCommand` (clap), `WmEvent`, IPC wire types, DTOs, `WindowState`/`DisplayState`, `try_warn!` |
| `wm-ipc-client` | lib | WebSocket client (`ws://127.0.0.1:6123`) used by CLI + watcher |
| `wm-macros` | proc-macro | `#[derive(SubEnum)]`, `#[derive(EnumFromInner)]` |
| `wm-platform` | lib | ALL Win32 access, behind a facade; `compile_error!` on non-Windows targets |
| `wm-watcher` | bin `lonewm-watcher` | Crash-recovery sidecar: tracks managed handles via IPC, restores (uncloak/show/taskbar/border/transparency) if the WM dies |

## Runtime topology

- **Main OS thread**: `EventLoop::run()` — `GetMessageW` pump over a hidden
  message window. The event loop is `!Send` and stays on the main thread
  (historically a macOS constraint; the architecture is unchanged).
- **Worker thread**: tokio runtime running `start_wm` (the select loop below).
- **`"keyboard-hook"` thread**: dedicated thread owning the `WH_KEYBOARD_LL`
  hook + its own message pump (isolated so event floods can't delay keystrokes).
- All platform calls from the WM go through `Dispatcher`
  (`dispatch_sync` with a 5s timeout / `dispatch_async` via PostMessage;
  closures are double-boxed to dodge a Windows access-violation quirk).

What pumps on the event-loop thread: raw input (`WM_INPUT`), WinEvent hooks
(`WINEVENT_OUTOFCONTEXT`), display/power messages (wndproc callbacks), all
dispatched closures. What does not: the keyboard hook.

## The `wm` crate: main loop (`wm/src/main.rs`, `start_wm`)

Startup order (matters): logging → `SingleInstance` → `UserConfig::new` →
`SystemTray` → `WindowManager::new` (builds `WmState` + `populate` + one
`platform_sync`) → `IpcServer::start` → watcher spawn → listeners (window,
display, mouse, keybinding — created **after** state population) → startup
commands → 5s cleanup interval.

`tokio::select!` branches: ctrl_c / `wm.exit_rx` / `tray.exit_rx` (all `break`)
· `mouse_listener.next_event` · `window_listener.next_event` ·
`display_listener.next_event` · `keybinding_listener.next_event` ·
`cleanup_interval.tick` (skips while paused; runs `cleanup_invalid_windows`;
re-dispatches display reconciliation if `needs_display_resync`) ·
`ipc_server.message_rx` · `wm.event_rx` (re-pushes keybinding map on
config/binding-mode/pause changes; forwards events to IPC subscribers) ·
`tray.config_reload_rx`.

`WindowManager::process_event` → `events::handle_*` → (unless paused)
`platform_sync` if `pending_sync.has_changes()`. The keybinding arm returns
early — `process_commands` already synced (avoid double flush).
`process_commands` resolves subject container → `run_commands` (re-resolves
the subject after each command in case the container was replaced/detached) →
`platform_sync`. **Invariant: exactly one `platform_sync` per batch, ending in
`pending_sync.clear()`.**

## Container tree (`wm/src/models/`)

```
RootContainer → Monitor → Workspace → (SplitContainer → …) → TilingWindow
                                        └→ NonTilingWindow (direct workspace child, ALWAYS)
```

- All nodes: `Rc<RefCell<Inner>>` with `id: Uuid`, `parent`, `children:
  VecDeque`, `child_focus_order: VecDeque<Uuid>` (most-recently-focused first).
  Single-threaded; cheap clones; `is_detached()` = no parent.
- Equality is **id-based**. Tiling↔NonTiling conversion preserves the UUID but
  creates a new node — old handles compare equal but are detached; always
  re-fetch after `update_window_state`.
- `Container` is an enum of the six node types; `wm_macros::SubEnum` generates
  `TilingContainer` (Split + TilingWindow), `WindowContainer` (both windows),
  `DirectionContainer` (Workspace + Split); ambassador `Delegate` forwards
  trait impls from enum to variants.
- Focus: `descendant_focus_order()` (DFS, leaves only); `focused_container()`
  = first of it from root. `set_focused_descendant` shifts the target to index
  0 of every ancestor's `child_focus_order`. ⚠️ verify: every tree mutation
  helper maintains `children` + `child_focus_order` together.
- `InsertionTarget` = remembered tiling slot (parent, index, prev size, sibling
  count) captured when leaving tiling; honored on re-tiling only if the target
  workspace is displayed; cleared when a floating window crosses monitors.

## State machines

**`WindowState`** (`wm-common`): `Floating(cfg)` | `Fullscreen{maximized,
shown_on_top}` | `Minimized` | `Tiling`. Model split, not a tag: Tiling lives
in `TilingWindow` nodes inside splits; the rest are `NonTilingWindow` direct
workspace children. Transitions only via
`commands/window/update_window_state.rs` (converts node, preserves id,
records `prev_state` + `InsertionTarget`, re-indexes in `WmState`).
`toggled_state` priority: target (different discriminant) → `prev_state`
(non-minimized) → config default → fallback. A native maximize maps to
`Fullscreen{maximized: true}`.

**`DisplayState`** (`wm-common`): `Shown | Showing | Hidden | Hiding` — async
show/hide handshake per window. `platform_sync::redraw_containers` sets
`Showing`/`Hiding` before repositioning; `handle_window_shown`/`hidden`
complete the transition (`Showing→Shown`, `Hiding→Hidden`). `HideMethod`:
`Cloak` (default; `IApplicationView::SetCloak`), `Hide` (`ShowWindowAsync`),
`PlaceInCorner` (reposition; no OS events, so the moved-or-resized handler
infers state via `is_in_corner`).

## WmState (`wm/src/wm_state.rs`) — field semantics

Key fields: `root_container` · `pending_sync` · `recent_workspace_name` (backs
`WorkspaceTarget::Recent` + `toggle_workspace_on_refocus`) ·
`prev_effects_window` (incremental effect updates) ·
`unmanaged_or_minimized_timestamp` (100ms focus-override window) ·
`binding_modes` (0 or 1) · `ignored_windows` · `is_paused` ·
`needs_display_resync` (retry flag) · `wm_set_frames: HashMap<Uuid, (Rect,
Instant)>` (echo suppression + 500ms minimize suppression) ·
`managed_timestamps` (1s launch-placement grace) · `windows_by_native_id`
(O(1) event lookup; private) · `managed_window_ids: Arc<Mutex<HashSet>>`
(shared with the platform hook for source-side filtering) ·
`can_manage_elevated` (token check at startup) · `is_focus_synced`.

Key methods: `populate` (manages existing windows in reverse z-order, picks
focus, first sync) · `window_from_native` (O(1)) · `index_window`/
`unindex_window` (maintain both maps — call at manage/unmanage/ignore/state
conversions) · `workspace_by_target` · `focus_target_after_removal` (never
falls back to a minimized window) · `cleanup_invalid_windows` (invalid handles
+ thread-less ghosts; prunes caches) · `monitors_by_hide_corner` ·
`emit_event` (suppressed until initialized and while paused, except
`PauseChanged`).

`Drop for WmState`: final restore — redraw to intended rects, uncloak, show,
taskbar, border, transparency reset. The watcher does the same on crash.

## Command surface

Tiling layout: LoneWM is a pure Dwindle window manager (BSP binary tree).
Dwindle insertion lives in
`commands/window/manage_window.rs::dwindle_insertion_target` — wraps the
focused tiling window in a depth-parity split; used by both new-window and
move-to-workspace paths. Directional movement within a workspace uses
`commands/container/swap_tiling_windows.rs` to atomically swap leaf positions
with the 2D spatial neighbor.

`InvokeCommand` (clap, in `wm-common/src/app_command.rs`) is the single
grammar for config YAML, CLI, and IPC strings (YAML values are re-parsed
through clap with a fake argv[0]). Dispatch: `wm/src/wm.rs::run_command` —
`Focus`/`Move` check optional flags in a fixed order; window-only commands
no-op on non-window subjects. Groups:

- `commands/container/` — tree primitives: attach/detach/replace/
  move_within_tree, resize_tiling_container, wrap_in_split_container,
  flatten*, set_focused_descendant, focus_in_direction, toggle_tiling_direction
- `commands/general/` — `platform_sync.rs` (the flush funnel), `reload_config`,
  `shell_exec`, `toggle_pause`, binding modes, `cycle_focus`
- `commands/monitor/` — add/remove/update/sort monitors, focus_monitor,
  move_bounded_workspaces_to_new_monitor
- `commands/window/` — manage/unmanage/ignore, update_window_state,
  move_in_direction/to_workspace, resize, set_position/size, run_window_rules
- `commands/workspace/` — activate/deactivate/focus, sort, update_config,
  move_in_direction

`PendingSync` (`wm/src/pending_sync.rs`): redraw queues, reorder list,
focus/effects/cursor-jump flags. Fixed flush order in `platform_sync`:
focus → redraw/reorder → cursor jump → effects.

## Event handlers (`wm/src/events/`)

`handle_window_moved_or_resized(.rs/_end.rs)` is the big heuristic handler:
echo dedupe, frame/shadow-border cache refresh, active-drag tracking (10px
float threshold; drag start/end via `EVENT_SYSTEM_MOVESIZE*` flags), corner-hide
inference, fullscreen/maximize detection (2px tolerances + `wm_set_frames`
echo suppression + 1s launch-maximize grace), floating placement updates.
Others: `focused` (100ms override, `Hiding` skip, hidden-window force-show →
workspace switch), `hidden`/`shown` (DisplayState completion; unmanage only on
true `WS_VISIBLE` loss), `minimized` (authoritative; 500ms post-reposition
suppression), `minimize_ended` (restore `prev_state`; taskbar re-assert),
`destroyed` (ignored-window prune + focus-override arm), `mouse_move`
(focus-follows-cursor via `dispatcher.window_from_point`; skipped while
buttons held or focus unsynced), `display_settings_changed` (monitor
reconciliation, retry-on-failure), `title_changed`.

## Platform layer (`wm-platform`)

Facade pattern: `src/lib.rs` re-exports per-module facades; `platform_impl`
is `pub(crate)` and Windows-only (`compile_error!` elsewhere). The OS surface
lives in extension traits — `NativeWindowWindowsExt`, `DispatcherExtWindows`,
`DisplayExtWindows`, `DisplayDeviceExtWindows` (the `Windows` suffix is
legacy; treat them as THE interface). Known leak: a few `windows`-crate
constants (`SWP_*`, `WS_*`, `WINDOW_STYLE`…) are re-exported from `lib.rs`
(TODO'd).

Highlights (`src/platform_impl/windows/`):
- `native_window.rs` — `focus()` tries `SetForegroundWindow` first, falls back
  to synthetic `SendInput` tagged `dwExtraInfo = 6379`
  (`FOREGROUND_INPUT_IDENTIFIER` — mouse_listener filters it back out);
  `set_cloaked` via undocumented `IApplicationView`; `set_taskbar_visibility`
  (`AddTab`/`DeleteTab`); `mark_fullscreen`; `is_elevated` (process token);
  `has_owning_thread` (ghost detection); `frame()` prefers
  `DWMWA_EXTENDED_FRAME_BOUNDS`.
- `com.rs` — STA COM thread-local, cached shell interfaces, `with_retry`
  (refresh once after Explorer restart).
- `window_listener.rs` — six `SetWinEventHook` ranges → `WindowEvent` mapping;
  `LOCATIONCHANGE`/`NAMECHANGE` dropped at the source for unmanaged windows
  (shared `managed_window_ids` set); two `OnceLock` thread-locals ⇒ max one
  `WindowListener` per event-loop thread.
- `keyboard_hook.rs` — dedicated thread; `GetAsyncKeyState` (NOT `GetKeyState`
  — hook thread has no message pump); swallows matched combos; arms
  Win-keyup swallow when a binding uses Win.
- `mouse_listener.rs` — raw input `RIDEV_INPUTSINK`; Move throttled to 50ms;
  own-input filtered by magic 6379.
- `display_listener.rs` — `WM_DISPLAYCHANGE` always emits; `WM_SETTINGCHANGE`
  only `SPI_SETWORKAREA`; `WM_DEVICECHANGE` only `DBT_DEVNODES_CHANGED`;
  power broadcast gates events while suspended and force-emits on resume
  (incl. `PBT_APMRESUMECRITICAL`).
- `models/key(.rs/_code.rs)` — `Key` enum (no Cmd variants — removed with
  macOS), `KeyCode(pub u16)` = virtual key; conversion table maps each `Key`
  to a `VK_*` constant.

## IPC & tray

`wm/src/ipc_server.rs`: TCP + WebSocket on `127.0.0.1:6123`
(`DEFAULT_IPC_PORT`). Text frames of JSON; grammar = `AppCommand::try_parse_from`.
`query …` / `command --id <uuid> …` / `sub -e <events…>` / `unsub --id …`.
Responses echo the exact command string (clients match on it). `Start` over
IPC is rejected. `wm/src/sys_tray.rs`: reload config, show config folder,
window animations, run-on-startup, exit.

## Key landmarks (path → responsibility)

- `wm/src/main.rs` — startup + select loop
- `wm/src/wm.rs` — `WindowManager`, command dispatch, pause gating
- `wm/src/wm_state.rs` — state, indexes, focus helpers, cleanup, Drop restore
- `wm/src/commands/general/platform_sync.rs` — THE flush funnel
- `wm/src/commands/window/{manage,unmanage,update_window_state}.rs` — lifecycle
- `wm/src/events/handle_window_moved_or_resized.rs` — heuristics hub
- `wm/src/traits/window_getters.rs` — `should_fullscreen`, `toggled_state`,
  fullscreen tolerances (`ENTER/KEEP_FULLSCREEN_TOLERANCE` = 2px)
- `wm/src/user_config.rs` — config discovery (`--config` →
  `LONEWM_CONFIG_PATH` (legacy `GLAZEWM_CONFIG_PATH`) → `~/.lonewm/config.yaml`,
  legacy `~/.glzr/glazewm/config.yaml` fallback), default ignore/float
  rules, `active_keybinding_configs`
- `wm-platform/src/dispatcher.rs` + `platform_impl/windows/event_loop.rs` —
  threading core
- `wm-platform/src/models/rect.rs` — rect math (`clamp`, `inset`,
  `contains_rect`, `translate_to_center`)
- `wm-common/src/parsed_config.rs` — config schema
- `wm-macros/src/lib.rs` — proc macros

## Gotchas (short list — more in conventions.md)

1. Non-tiling windows are always direct workspace children (enforced).
2. `InsertionTarget` validity is time-sensitive (target workspace displayed).
3. Magic timing windows: 100ms focus override, 1s launch grace, 500ms minimize
   suppression, 50ms border re-apply, 10px drag threshold, 2px fullscreen
   tolerance.
4. Pause is escape-proof: only `WmTogglePause` keybindings survive; cleanup
   force-unpauses for shutdown commands.
5. Monitor identity is fuzzy: handle → device path → hardware id (only if
   unique); never remove the last monitor.
6. `dispatch_sync` has a 5s timeout; nested calls on the loop thread run inline.
7. `wm-watcher` build.rs panics off-Windows (kept as a guard, though the whole
   workspace is Windows-only now).
8. CI is Windows-only: lint (fmt + clippy `-D warnings`), build (x64 + ARM64
   msvc), package (WiX + AzureSignTool), release, winget.
