# Worklog & upstream-issue knowledge base

## Cleanup + Dwindle Stabilization + Border Hardening (2026-08-26)

Three-part change set (11 files), all gates green.

**Part A — Final cross-platform remnants removed:**
- `wm-platform/src/platform_impl/mod.rs`: removed redundant `cfg` gates.
- `wm-watcher/build.rs`: removed `cfg!(not(windows))` panic guard.
- `wm/src/sys_tray.rs`: removed 2× `cfg_attr` on `animations_enabled`.
- `wm-platform/src/keybinding_listener.rs`: fixed `Key::Cmd` → `Key::LWin`
  in doc comment.
- `wm-platform/src/dispatcher.rs`: removed "cross-platform" wording and
  stale `shell_util` TODO.

**Part B — Dwindle stabilization (3 bugs fixed):**
1. `move_window_to_workspace.rs`: sibling lookup changed from
   `.state() == WindowState::Tiling` to `Container::is_tiling_window` —
   fixes tree corruption when moving windows to workspaces with
   fullscreen/maximized tiling windows.
2. `manage_window.rs::dwindle_insertion_target`: direction now derived from
   `parent.tiling_direction().inverse()` instead of workspace-root +
   depth-count — respects manual `toggle-tiling-direction` overrides.
3. `traits/tiling_size_getters.rs::container_to_resize`: rewritten to walk
   up the ancestor chain instead of only checking parent/grandparent —
   correct resize axis after manual direction toggles.

**Part C — Border hardening:**
- `toggle_pause.rs`, `reload_config.rs`, `wm_state.rs` Drop: border resets
  now check `border_stamp_cache` before calling `set_border_color(None)` —
  avoids redundant `DwmSetWindowAttribute` calls that cause frame flicker
  on Electron/JetBrains apps.
- `platform_sync.rs::apply_corner_effect`: added `corner_style` field to
  `BorderStamp` and skip-if-unchanged caching (same pattern as borders).
- `platform_sync.rs`: shadow border cache refreshed after DPI
  double-`SetWindowPos` adjustment.

**Part D — Config defaults:**
- `wm-common/src/parsed_config.rs` (`GeneralConfig::default()`),
  `sample-config.yaml`, `README.md`, and `README_zh.md`: enabled
  `focus_follows_cursor: true` by default.

**Part E — DWM focused border fix (delayed re-stamp):**
- `wm/src/wm_state.rs`: added `focus_generation: Arc<AtomicU64>` to `WmState`
  to monotonically track focus transitions.
- `wm/src/commands/general/platform_sync.rs`: added 50ms async delayed
  re-stamp task in `apply_window_effects` so `DWMWA_BORDER_COLOR` is
  re-applied after modern apps (Windows Terminal, Chrome/Electron, WinUI 3)
  finish their asynchronous `WM_NCACTIVATE(TRUE)` / DWM frame reset routines.
- Cleaned up diagnostic hooks from `native_window.rs`, `window_listener.rs`,
  `handle_window_focused.rs`, and `platform_sync.rs`.

## Rebrand to LoneWM (2026-08-22)

Full rename from GlazeWM: binaries `lonewm{,-cli,-watcher}` (Cargo `[[bin]]`
names, build.rs version resources, watcher spawn path, CLI relauncher sibling
lookup); display name "LoneWM" (tray tooltip, AutoLaunch entry, clap app name
`lonewm`); config `~/.lonewm/config.yaml` with legacy `.glzr/glazewm` path +
`GLAZEWM_CONFIG_PATH` env fallbacks (`user_config.rs::default_config_path`);
error log `~/.lonewm/errors.log`; new single-instance mutex GUID; message
window message name `LoneWM:Dispatch`; WiX (product, install dir
`Program Files\LoneWM`, registry keys, starter props) + `package.ps1` exe
names; release artifacts `lonewm-v*`; winget workflow removed (can't publish
to glzr-io); README/README_zh/CONTRIBUTING/.vscode rebranded; repo URLs point
to `Louis047/LoneWM`. Upstream issue URLs in code comments intentionally
retained for attribution.

## Dwindle layout (2026-08-22)

Semantics (from research across dwm-fibonacci/bspwm/Hyprland; research trail
in session log): new/moved tiling window **splits the focused tiling window**
(bspwm/Hyprland model, not dwm's order-based chain), 50/50 ratio via existing
`attach_container` sizing, split direction = workspace's live
`tiling_direction` inverted `depth` times (depth = split ancestors between
sibling and workspace) → H,V,H… spiral to bottom-right. Splits persist
(Hyprland `preserve_split` behavior by construction); removal = existing BSP
collapse; `toggle-tiling-direction` remains the manual override; rects need
no changes (derive from parent directions).

Implemented:
- `wm-common/src/tiling_layout.rs` — `TilingLayout { Standard, Dwindle }`,
  **Default = Dwindle** (per owner decision), clap `ValueEnum`, serde
  snake_case.
- `WorkspaceConfig.layout` (serde default → Dwindle) +
  `InvokeUpdateWorkspaceConfig.layout` + `update_workspace_config` merge +
  `WorkspaceDto.layout` (IPC/status bar visibility) + `Workspace::mock` builder
  arg.
- `manage_window.rs::dwindle_insertion_target(sibling, gaps)` (pub(crate)):
  computes parity direction, wraps sibling via `wrap_in_split_container`,
  returns `(split, 1)` — consumed by `insertion_target` (new windows) and
  `move_window_to_workspace` (moved windows).
- Sample config documents `layout: 'dwindle'` (workspace 1).
- Tests: `manage_window.rs::dwindle_tests` — 5 structure tests over mock
  trees (H-first, V-second, H-third, vertical workspace variant, 50/50
  sizes). All gates green.

Known nuances: parity derives from the workspace's **live** direction, so
toggling the workspace direction re-bases future insertions' parity;
`layout` changes take effect for subsequent insertions only (existing trees
untouched, by design — persistent splits).

---

Previous research trail (2026-08-21): scraped all 149 open `type: bug` issues from `glzr-io/glazewm`
(2026-08-21), shortlisted ~45 Windows-OS-specific ones into 9 clusters,
root-caused each in this codebase, then implemented fixes in 6 phases.
Status: **implemented + green (build/test/clippy/fmt), uncommitted**.

Gates at time of writing: `cargo check/build/test --workspace` clean (except
pre-existing `wm-macros` doctests), `cargo clippy --workspace` 0 warnings,
`cargo fmt` applied. 32 files changed, +~1300/−265 lines.

## Phase 1 — small fixes

| Issue(s) | Root cause | Fix (where) |
|---|---|---|
| #1370 pause still intercepts hotkeys | `main.rs` re-pushed keybinding map with hardcoded `is_paused: false` | pass `wm.state.is_paused` (`wm/src/main.rs`) |
| #958 effects frozen while paused | `toggle_pause` did no cleanup; `platform_sync` skipped while paused; unpause didn't queue effects | reset border/title-bar/corner/transparency on pause; `queue_all_effects_update` on unpause (`commands/general/toggle_pause.rs`) |
| #1394 taskbar shows all workspaces' apps | `AddTab`/`DeleteTab` only called during `Showing/Hiding` transitions; shell silently re-adds buttons; reload handled only the `true` direction | assert idempotently every redraw + both reload directions (`commands/general/platform_sync.rs`, `reload_config.rs`) |
| #1411 floats recentered on USB change | `WM_DEVICECHANGE` treated as display change; recenter pass used gapped `to_rect()` with zero-tolerance intersection | skip reconciliation when displays unchanged (`displays_unchanged`); intersect vs `max_workspace_rect()` (`events/handle_display_settings_changed.rs`) |
| #1233 monitor re-add needs restart / #1381 sleep-wake multi-monitor | one flaky display-property query aborted whole reconciliation (`try_warn!` early-return); `PBT_APMRESUMECRITICAL` unhandled; swallowed events never replayed | per-display fallible + `needs_display_resync` retry on 5s tick; emit display change on any resume (`wm-platform/.../display_listener.rs`, `wm/src/main.rs`) |
| #978 taskbar previews steal focus | preview host windows passed `check_is_manageable` | explorer + class-name blacklist (`TaskListThumbnailWnd`, `XamlExplorerHostIslandWindow`, …) (`commands/window/manage_window.rs`) |

## Phase 2 — cloaking / workspaces

| Issue(s) | Root cause | Fix |
|---|---|---|
| #1350 apps leak through workspaces / #992 duplicated across workspaces | `handle_window_hidden` unmanaged any `Shown` window with `!is_visible()` — but `is_visible()` conflates DWM-cloak (suspended UWP/Electron) with hidden; window fell out of tree, re-managed onto wrong workspace | new `is_shown()` API (raw `WS_VISIBLE`); unmanage only on true hide (`events/handle_window_hidden.rs`, `wm-platform` `NativeWindowWindowsExt`) |
| #1358 minimize → permanently invisible | minimized+cloaked windows never restored; `AddTab` fails silently on them; exit/ignore never uncloaked | taskbar re-assert on minimize-end (`events/handle_window_minimize_ended.rs`); uncloak in `WmState::drop`, watcher cleanup, `ignore_window`, and hide-method reload |
| #860 sporadic switch failures / #869 ignored-window close switches workspace | stale focus events for `Hidden` windows; z-order auto-pick after ignored window close armed no override | switch workspace only if window natively shown; arm `unmanaged_or_minimized_timestamp` on ignored-window destroy (`events/handle_window_focused.rs`, `handle_window_destroyed.rs`) |

## Phase 3 — fullscreen state machine

| Issue(s) | Root cause | Fix |
|---|---|---|
| #697 tiling breaks with small top gap / #856 move→monitor goes fullscreen / #1418 float oscillation + CPU / #1013 stuck fullscreen | `should_fullscreen` used `frame.inset(1).contains_rect(ws)` — 1px tolerance inside OS/DWM noise → misclassification + fullscreen↔floating loops | `ENTER/KEEP_FULLSCREEN_TOLERANCE` = 2px (`traits/window_getters.rs`); echo suppression via `wm_set_frames` map (`events/handle_window_moved_or_resized.rs`, `commands/general/platform_sync.rs`); clamp floating placements on cross-monitor moves (`move_window_to_workspace.rs`, `add_monitor.rs`, `handle_display_settings_changed.rs`) |
| #1365 apps open maximized / #1165 stuck max-min cycle | app self-maximize right after manage treated as user maximize; `prev_state` overwrote with `Fullscreen{maximized}` | `managed_timestamps` 1s grace: launch maximize → tiling restore; minimize-ended coerce for recent manages (`handle_window_moved_or_resized.rs`, `handle_window_minimize_ended.rs`) |
| #1015 tiny size on fullscreen exit / #996 overlaps bar / #737 wrong size | `to_non_tiling` carried stale manage-time `floating_placement`; exit redraw used stale shadow-border cache | placement defaults to current tiling rect (border-delta applied) (`models/tiling_window.rs`); shadow-border refresh in `update_window_state` |
| #682 stuck fullscreen at WM start / #833 F11 fullscreen fights WM | manage-time fullscreen had `prev_state: None` (no `MarkFullscreenWindow`, no exit path); redraw forced `SetWindowPos` on app-fullscreen windows | seed `prev_state` at manage; idempotent `mark_fullscreen`; skip reposition when frame already covers monitor (`manage_window.rs`, `platform_sync.rs`) |

## Phase 4 — input & focus

| Issue(s) | Root cause | Fix |
|---|---|---|
| #1215 Win-key opens Start menu | hook only swallowed keypresses; Win keyup reached Explorer | `swallow_win_up` flag armed on matched Win-combos; swallow subsequent Win keyup (`wm-platform/src/keybinding_listener.rs`) |
| #1019 accidental mouse move/click | `focus()` unconditionally injected `SendInput` (observable by remote-input tools); cursor jump mid-click | `SetForegroundWindow` first, inject as fallback; skip cursor jump while button down (`wm-platform/.../native_window.rs`, `platform_sync.rs` `jump_cursor`) |
| #1115 focus dead after minimize | `MINIMIZESTART` gated on synchronous `IsIconic` (false at event time for slow apps); minimized windows remained focus targets | treat event authoritative; 500ms post-reposition verification guard; no minimized focus fallbacks (`events/handle_window_minimized.rs`, `wm_state.rs`) |
| #1259 cross-monitor move → minimized | transient minimize during WM-initiated restore+move latched `Minimized` | same 500ms guard (see above) |

## Phase 5 — elevated windows (#867, #1041)

Without UIAccess, elevated windows get managed but every `SetWindowPos`/
focus silently fails (UIPI) — phantom layout slots. Fix: `is_elevated()`
(target process token), `can_manage_elevated_windows()` (self: elevation or
UIAccess token), exclude+warn+ignore in `manage_window`, README FAQ
documenting `ui_access` feature (signing + secure location requirements).

## Phase 6 — performance / architecture

| Item | Change |
|---|---|
| O(1) window lookup (#1225) | `windows_by_native_id: HashMap<WindowId, WindowContainer>` + `index_window`/`unindex_window` at manage/unmanage/ignore/state-conversion sites (`wm_state.rs`) |
| Event-flood reduction (#1225/#1020) | `LOCATIONCHANGE`/`NAMECHANGE` dropped at the WinEvent hook for unmanaged windows via shared `managed_window_ids: Arc<Mutex<HashSet<WindowId>>>` (`wm-platform/.../window_listener.rs`) |
| Keyboard latency | hook moved to dedicated `"keyboard-hook"` thread with own pump; `GetAsyncKeyState` replaces `GetKeyState` (also fixes #1154 modifier lag) |
| Ghost windows (#1225) | cleanup also prunes valid-handle-but-no-thread windows (`has_owning_thread`) |
| DPI (#661 partial) | `has_pending_dpi_adjustment` set on floating cross-monitor moves (double-`SetWindowPos` hack runs) |
| Named-pipe IPC (#1020) | **deliberately skipped**: reporter retracted; switching transport breaks TCP WebSocket clients (wm-cli, status bars, scripts) |

## Upstream issue clusters — status map

Research trail (keep for future triage; ⚠️ verify against upstream before
citing — statuses drift):

- **Fixed here**: 697, 833, 856, 860, 867, 869, 958, 992, 996, 1013,
  1015, 1019, 1020(partial/perf), 1041, 1115, 1154, 1165, 1215, 1225, 1233,
  1259, 1350, 1358, 1365, 1370, 1381, 1394, 1411, 1418, 978.
- **Not addressed (open)**: 661 (DPI sizing on overlapping monitors — proper
  `WM_DPICHANGED` handling needs window subclassing, deemed too invasive);
  1020 full fix (named-pipe IPC, skipped for compat); app-specific tiling
  bugs (1212/1095 Zen, 1126 Office tabs, 1195 Explorer tabs, 1283 Tcl/Tk,
  1179/1011 VMware, 1136 VirtualBox, 1010 Unity, 977/914 WSLg, 862 mstsc —
  mostly need per-app window-rule investigation, not WM core changes);
  1225's residual O(N) paths; 1415 installer panel (packaging).
- **Fix strategy notes**: any new window-lifecycle bug should be triaged
  against `wm_set_frames`/`managed_timestamps` guards first — most
  historical oscillation bugs are self-inflicted event echoes.

## Windows-only migration (2026-08-22)

Complete removal of macOS/cross-platform support, per owner decision.
Verified green after each phase (check/build/test/clippy `-D warnings`/fmt).

What changed:

1. **wm-platform**: deleted `platform_impl/macos/**` (15 files);
   `platform_impl/mod.rs` now Windows-only with a `compile_error!` guard;
   `build.rs` emptied (framework link line gone); objc2 dependency block and
   `libtest-mimic-collect` dev-dep removed from Cargo.toml; `windows` crate
   moved from target-gated to plain `[dependencies]`. Facades unwrapped:
   `DisplayExtMacOs`/`DisplayDeviceExtMacOs`/`NativeWindowExtMacOs`/
   `DispatcherExtMacOs` deleted; `DisplayId`/`WindowId`/`KeyCode` are plain
   (`isize`/`isize`/`u16`); macOS Error variants dropped; dispatcher methods
   (cursor, mouse-down, set-cursor, file explorer, error dialog) written
   Windows-direct. **Test harness reverted to standard libtest** (`[lib] test`
   re-enabled, `src/test.rs` deleted, no `--test-threads=1` requirement — the
   harness only existed for macOS main-thread APIs).
2. **Keys**: `Cmd`/`LCmd`/`RCmd` variants removed from `Key`, parsing tables,
   keycode tables (now a flat `Key => VK_*` macro), and `WIN_KEYS`/
   `MODIFIER_GROUPS`. Configs using `cmd` now fail to parse — use `win`.
   `KeyCode` conversion macro rewritten Windows-only.
3. **wm crate**: all `#[cfg]` branches removed (drag detection via
   `EVENT_SYSTEM_MOVESIZE*` only, `is_shown()` paths unconditional,
   macOS AXWindow manageability check gone, `update_path_env` +
   accessibility check gone from `main.rs`, `shell_exec` Windows-direct,
   `sys_tray` simplified, `non_tiling_window` fullscreen rect = monitor
   bounds, `NativeMonitorProperties::device_uuid` removed,
   `NativeWindowProperties.class_name`/`shadow_borders` unconditional,
   `MonitorDto` handle/path/hardware_id always populated). `shell-util`
   dependency removed.
4. **wm-common**: `deserialize_hide_method` no longer rewrites to
   `PlaceInCorner`; default `hide_method: Cloak`; `WindowDto.class_name`
   unconditional. `MouseEvent::Move { window_below_cursor }` field removed
   (was always `None` on Windows).
5. **CI/packaging**: `lint-check.yaml` windows-only (and now lints
   `--workspace`); `build.yaml` windows-only matrix (x64 + ARM64 msvc);
   `package.yaml` macOS job deleted; `release.yaml` dmg artifacts removed;
   `resources/Info.plist` deleted. `package.ps1` untouched (already
   Windows-only).
6. **Workspace/docs**: `default-members` now includes `wm-watcher`;
   CONTRIBUTING.md + sample-config.yaml macOS mentions cleaned. All macOS
   doc-comment mentions swept from sources.

⚠️ verify before merging: nothing — all phases compile/test clean on this
machine (GNU toolchain). CI (msvc) should behave identically; watch the first
CI run.

## Open follow-ups

1. Commit both change sets (suggest one commit per phase; Conventional
   Commits, e.g. `feat!: drop macOS support` / `fix: ...` per cluster) —
   awaiting owner decision.
2. Manual verification matrix on a real multi-monitor Windows box (mixed DPI,
   taskbar top/auto-hide, USB hotplug, sleep/wake, elevated apps, F11 apps,
   UWP suspend/resume) — fixes are conservative but several address
   runtime-only behaviors.
3. Consider upstreaming high-confidence fixes (P1 items especially).
4. `SingleInstance::is_running` inverted-logic suspicion (latent).
5. `windows`-crate constant re-export leak in `wm-platform/src/lib.rs`
   (upstream TODO).
