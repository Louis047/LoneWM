# Conventions & gotchas

Everything a contributor (human or AI) needs to write code that matches the
codebase. Sources: CLAUDE.md/AGENTS.md rules, CI config, code audit.

## Style

- rustfmt (nightly-only options): `tab_spaces = 2`, `max_width = 75`,
  `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`,
  `wrap_comments = true` — run `cargo fmt` before committing; CI runs
  `cargo fmt --check`.
- Import groups: (1) `std::…`, (2) external crates (alphabetical), (3)
  `use crate::…` last. `#[cfg(target_os)]`-gated imports sit inline in their
  group.
- Files snake_case named after the primary symbol; `mod.rs` for folders;
  pattern is `mod x;` + `pub use x::*;` (wm crate) or `pub mod` (commands).
- `.editorconfig`: UTF-8, 2-space, final newline, trim trailing whitespace.

## Errors & logging

- `anyhow` + `.context("…")` in every crate **except `wm-platform`**, which
  uses thiserror `crate::Error`/`crate::Result` (`wm-platform/src/error.rs`).
- `try_warn!` (wm-common, `#[macro_export]`): on `Err` logs
  `tracing::warn!("Operation failed: {:?}")` and **early-returns `Ok(())`**
  from the enclosing fn — for recoverable native failures in handlers.
- Avoid `.unwrap()` in non-test code (tests may use it).
- `tracing`: `info!` lifecycle/user-visible actions, `warn!` recoverable
  failures, `debug!` detail, `error!` fatal. Subscriber initialized once in
  `wm/src/main.rs`. Fully-qualified `tracing::info!(…)` and `use tracing::info`
  both exist in the codebase.

## Doc comments & comments

- Document every function/type: one-line summary; optional caveats; optional
  "Returns …"; optional `# Example usage` (mark non-compiling ones
  ```ignore`); optional `# Platform-specific` bullets.
- Backtick type names in comments; end all comments with punctuation;
  `SAFETY:` comments on unsafe blocks; `// NOTE:` / `// TODO:` / `// LINT:`
  inline markers (e.g. `// LINT: \`z_order\` is only used on Windows.`).
- clippy.toml extends `doc-valid-idents` with `AppKit`, `DisplayPort`.

## Platform code

- The codebase is **Windows-only**. All Win32 usage stays inside
  `wm-platform/src/platform_impl/**` (`pub(crate)`). Other crates use the
  facade and the ext traits (`NativeWindowWindowsExt`,
  `DispatcherExtWindows`, …). `wm-platform` has a `compile_error!` guard on
  non-Windows targets — don't add `#[cfg(target_os)]` branches back; write
  the Windows path directly.
- No `#[cfg(target_os)]`/`#[cfg_attr(not(windows))]` remains in the tree;
  if a param or import is Windows-only, it's just a normal param/import now.

## Clippy discipline

- Gate: `clippy::all` + `clippy::pedantic` (workspace `[workspace.lints]`
  exists but packages do NOT inherit it — no `[lints] workspace = true`
  anywhere; each crate root re-declares `#![warn(clippy::all,
  clippy::pedantic)]` + `#![allow(clippy::missing_errors_doc)]`; verified
  2026-08-22). If adding a crate, add the crate-root attributes.
- Suppress narrowly at item level. Common allows by frequency:
  `cast_possible_truncation` (casts i32/u16 everywhere), `unnecessary_wraps`,
  `too_many_lines` (>100 lines), `needless_pass_by_value`,
  `cast_precision_loss`, `struct_excessive_bools`, `missing_panics_doc`.
- CI equivalent: `cargo clippy --all-targets --all-features -- -D warnings`.

## Macro machinery (know before touching models/)

- `wm_macros::SubEnum` on `Container` generates `TilingContainer`,
  `WindowContainer`, `DirectionContainer` + `From`/`TryFrom` conversions
  between main and sub-enums (and across sub-enums for shared variants).
  `#[subenum(defaults, { … })]` applies shared derives/delegates.
- `wm_macros::EnumFromInner` generates `From<T> for Enum` / `TryFrom<Enum>
  for T` (+ refs) for single-field tuple variants.
- `ambassador::Delegate` + `#[delegatable_trait]` let the enums forward trait
  calls to variants. The five traits: `CommonGetters`, `PositionGetters`,
  `WindowGetters`, `TilingSizeGetters`, `TilingDirectionGetters`
  (`wm/src/traits/`), each with an `impl_*!` macro for the `Rc<RefCell<Inner>>`
  shape (macro assumes specific field names — new model fields need the macro
  updated or a manual impl).
- `impl_container_debug!` derives `Debug` via `to_dto()`.

## Testing patterns

- Tests are in-file `#[cfg(test)] mod tests`. Pure-logic only unless using
  mocks. CI does NOT run tests — run them yourself:
  - `cargo test -p wm` — uses `wm/src/test_utils.rs` bon-builders:
    `Monitor::mock()`, `Workspace::mock()`, `TilingWindow::mock()`, etc., with
    `MOCK_*` constants (1680×1050 monitor, 96 DPI, …). Never call native
    methods on mock handles (UB by contract).
  - `cargo test -p wm-platform` — standard libtest (the old
    `libtest_mimic_collect` main-thread harness was removed with macOS; no
    `--test-threads=1` flag needed).
- `wm-platform` `test_utils` feature: `Dispatcher::mock()`,
  `NativeWindow::mock()`, `Display::mock()`; pulled in as a dev-dependency by
  `wm`.

## Change recipes (where things must be updated in tandem)

- **New tree mutation**: maintain `children` + `child_focus_order` together;
  flatten emptied splits; emit relevant `WmEvent`s.
- **Window state transitions**: only via `update_window_state` — node is
  replaced (same UUID): afterwards re-index (`state.index_window`) and
  re-resolve any held `Container` handles (`is_detached()` check pattern in
  `run_commands`/`run_window_rules`).
- **New config option**: `wm-common/src/parsed_config.rs` (+ serde defaults) →
  sample config `resources/assets/sample-config.yaml` → consumers; emit
  nothing before `has_initialized`.
- **New WmEvent**: `wm-common/src/wm_event.rs` → `SubscribableEvent` mapping
  in `ipc_server.rs::process_event` → DTO if it carries containers.
- **New platform capability**: implement in `platform_impl/windows/`, expose
  via the facade (+ ext trait), never leak `windows` crate types (existing
  `SWP_*` leak is TODO'd, don't add more).
- **New keybinding-aware state**: update `active_keybinding_configs`
  consumers AND the `main.rs` re-push on
  `UserConfigChanged`/`BindingModesChanged`/`PauseChanged`.

## Known pitfalls

1. `GetKeyState` vs `GetAsyncKeyState`: on threads without a message pump
   (e.g. the keyboard hook thread), `GetKeyState` lags physical state — use
   `GetAsyncKeyState`. (Historical bug: win-key combos misfiring.)
2. COM: STA init per thread; `ComInit::new` panics on incompatible re-init;
   always go through `COM_INIT.with(...).with_retry(...)`.
3. `ShowWindowAsync` events arrive late — never assume ordering between
   DisplayState transitions and OS events; the `Showing/Hiding` handshake
   exists for this reason.
4. `is_visible()` conflates cloak; use `is_shown()` (raw `WS_VISIBLE`) when
   distinguishing shell-cloaked-but-alive windows (suspended UWP).
5. Synthetic input (SendInput in `focus()`) is system-wide observable — keep
   it as a fallback, keep the 6379 tag, keep the mouse-listener filter.
6. `windows` crate 0.52 quirks: `SetWindowsHookExW` returns `Result<HHOOK>`;
   attributes on expressions are unstable (put `#[allow]` on the fn);
   `GetKeyState`-style SHORT down-checks work via i16 sign extension.
7. Unbounded channels everywhere — don't add per-pixel/keystroke senders
   without throttling or source-side filtering.
8. `wm-macros` is edition-2024 proc-macro on nightly — its doctests are
   currently broken (pre-existing); mark doc examples `ignore` if adding any.
