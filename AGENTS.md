# AGENTS.md — LoneWM Project Brain (entry point)

LoneWM is a fork of **GlazeWM** (`glzr-io/glazewm`): a tiling window manager
inspired by i3wm, written in Rust — now **Windows-only** (all macOS/cross-
platform support was removed; `wm-platform` compiles a `compile_error!` on any
non-Windows target) and **rebranded to LoneWM** (binaries `lonewm`,
`lonewm-cli`, `lonewm-watcher`; config at `~/.lonewm/config.yaml` with a
legacy `~/.glzr/glazewm` fallback; env `LONEWM_CONFIG_PATH`, legacy `GLAZEWM_CONFIG_PATH` still honored). LoneWM is a **pure Dwindle
window manager** (new windows split the focused window in an alternating
spiral with 2D spatial swapping) — see `.brain/worklog.md` before
touching insertion logic. The fork's focus is **Windows stability** — see
`.brain/worklog.md` before touching window-lifecycle, focus, or fullscreen
code, because much of it carries deliberate, issue-driven logic.

This file is the always-loaded index. Topic files live in `.brain/` and are
**load-on-demand**: read the relevant one before working in that area.

## Brain index

| File | Read before working on |
|---|---|
| `.brain/architecture.md` | Anything structural: crates, container tree, event/command flow, state machines, platform layer, file landmarks |
| `.brain/conventions.md` | Writing/reviewing any code: style, error handling, platform patterns, macro machinery, pitfalls |
| `.brain/environment.md` | Building, testing, linting on this machine; toolchain quirks; pre-existing failures |
| `.brain/worklog.md` | Window lifecycle/focus/fullscreen/taskbar behavior; also holds the upstream-issue knowledge base and deferred work |

## Non-negotiable rules (summary — details in `.brain/conventions.md`)

1. **Nightly toolchain only** (`rust-toolchain.toml`; uses `iterator_try_collect`,
   `proc_macro_diagnostic`, unstable rustfmt options).
2. **Windows-only**: never call Win32 APIs outside `wm-platform`. All platform
   access goes through the facade + `*ExtWindows` extension traits (the `Ext`
   suffix is legacy naming — there is no other platform anymore, but the
   pattern is still the law).
3. **Error handling:** `anyhow` everywhere except `wm-platform`
   (`thiserror`-based `crate::Error`/`crate::Result`). `try_warn!` = log + early
   `return Ok(())`. Avoid `.unwrap()` in non-test code.
4. **Formatting:** 2-space indent, width 75, crate-granular imports grouped
   std → external → `crate`. `cargo fmt` before committing.
5. **Clippy gate:** `cargo clippy --workspace --all-targets --all-features --
   -D warnings` under `clippy::all` + `clippy::pedantic` (only
   `missing_errors_doc` allowed). Suppress narrowly, item-level.
6. **Document every function/type** (summary, caveats); backtick type names;
   end comments with punctuation; `SAFETY:` on unsafe.
7. **The event loop runs on the main OS thread** (kept architecture, not an OS
   constraint anymore). Tokio work runs on a spawned worker thread; platform
   calls go through `Dispatcher`.
8. **Tandem bookkeeping:** `children` + `child_focus_order` move together;
   Tiling↔NonTiling conversions preserve the window UUID but replace the node —
   re-index (`state.index_window`) and re-resolve container handles afterward.
9. **One `platform_sync` flush per event/command batch**, never while paused.
10. **Conventional Commits** for PR titles (`feat:`, `fix:`, …) — enforced by CI.

## Quick commands (this machine — see `.brain/environment.md` for caveats)

```bash
export PATH="$HOME/.cargo/bin:$HOME/scoop/apps/mingw/current/bin:$PATH"
cargo check --workspace                 # fast type check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all
cargo test --workspace --tests          # unit tests (skips broken wm-macros doctests)
```

## Fact-verification convention

Code facts in `.brain/` reference **file paths + symbol names** (no line
numbers, which drift). Anything likely to drift carries a `⚠️ verify:` marker —
re-confirm it in the code before relying on it. When you change behavior that a
brain file describes, update that file in the same commit.

## Current state snapshot

- Two large bodies of work are complete, green, and **uncommitted**: the
  Windows-stability overhaul (30+ upstream issues fixed) and the Windows-only
  migration (macOS platform code, cfg branches, CI jobs, and docs removed; the
  `wm-platform` test harness reverted to standard libtest) — see
  `.brain/worklog.md` for both change maps.
- Known pre-existing failures: `wm-macros` doctests fail on the current nightly
  (fail on the pristine tree too); `SingleInstance::is_running` looks inverted
  vs its doc (latent, unexercised).
- Removed surface (do not reintroduce): `Cmd`/`LCmd`/`RCmd` key aliases (use
  `win`/`lwin`/`rwin`), `MouseEvent::Move { window_below_cursor }`,
  `NativeMonitorProperties::device_uuid`, `deserialize_hide_method`'s macOS
  rewrite, `shell-util` dependency, macOS CI/packaging.
