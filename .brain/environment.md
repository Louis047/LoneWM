# Environment & build (this machine)

Machine: Windows 11 (10.0.26200), Git Bash shell. Repo at
`C:\Users\Lone\Documents\git-clones\LoneWM` (fork `Louis047/LoneWM` of
`glzr-io/glazewm`, branch `main`).

## Toolchain (installed 2026-08-21 for this project)

No system-wide Rust existed. Installed user-scoped, no admin needed:

1. **rustup, nightly GNU host** — `rustup-init.exe -y --default-host
   x86_64-pc-windows-gnu --default-toolchain nightly --profile minimal
   --no-modify-path`. Lives in `~/.cargo`. GNU host ships its own linker, so
   **no Visual Studio / MSVC / Windows SDK is required**.
2. **mingw-w64 via scoop** (`scoop install mingw`) — provides `windres.exe`,
   required by the `tauri-winres` build scripts of `wm`, `wm-cli`,
   `wm-watcher` when building with the GNU toolchain.
3. **clippy + rustfmt components** (`rustup component add clippy rustfmt`).

Every shell needs (profile does NOT add these):

```bash
export PATH="$HOME/.cargo/bin:$HOME/scoop/apps/mingw/current/bin:$PATH"
```

`rust-toolchain.toml` pins `channel = "nightly"` (floating). Nightly features
in use: `iterator_try_collect`, `proc_macro_diagnostic`, unstable rustfmt
options (`imports_granularity`, `group_imports`, `wrap_comments`) — do not
build with stable.

## Commands

```bash
cargo check --workspace                                   # fast type check
cargo build --workspace                                   # full build (works, GNU)
cargo test --workspace --tests                            # unit tests only
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

All currently green except the known failures below. `wm-platform` uses the
standard libtest harness now (the macOS main-thread harness was removed), so
plain `cargo test -p wm-platform` works — no `--test-threads=1` needed.

## Known pre-existing failures (do not chase these)

- **`wm-macros` doctests fail** on the current nightly (`expected item after
  attributes` in `sub_enum` doc examples; also a `syn::Type: Debug` bound
  error). Verified failing on the pristine tree (git stash check). Upstream
  has a history of this (`test: mark non-compiling doc tests as ignore
  #1356`). Use `--tests` to skip doctests.
- **`SingleInstance::is_running`** (windows impl) appears to return the
  inverse of its doc comment — latent; not called in practice. ⚠️ verify
  before any use.

## CI parity (what GitHub runs — `.github/workflows/lint-check.yaml`)

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (windows-latest, nightly + clippy/rustfmt; CI targets **msvc**, locally we
  use **gnu** — code is toolchain-family agnostic but release packaging
  (signing, WiX) is CI-only)
- CI does **not** run `cargo test`.
- Builds (`build.yaml`): windows-only matrix (x64 + ARM64 msvc),
  `cargo build --locked --release --target <triple> --workspace
  [--features ui_access]`.
- `pr-title-check.yaml`: PR titles must be Conventional Commits.
- Version: `VERSION_NUMBER` env (`.cargo/config.toml` default `0.0.0`);
  never hardcode versions.

## Config & run

- WM reads config from `--config` → `$GLAZEWM_CONFIG_PATH` →
  `~/.glzr/glazewm/config.yaml` (auto-created from
  `resources/assets/sample-config.yaml` on first run).
- Running the WM replaces your current shell's window management — test in a
  throwaway session or VM. `glazewm-watcher` must be built alongside
  (`cargo build -p wm-watcher`) or the watcher-start warning appears.
- Logs: stdout (verbosity flags) + `~/.glzr/glazewm/errors.log` (ERROR only).
- IPC: TCP WebSocket on `127.0.0.1:6123` — `glazewm-cli query windows` works
  against a running instance.

## Repo state notes

- Two large uncommitted change sets sit on `main` (see `.brain/worklog.md`):
  the Windows-stability overhaul and the Windows-only migration. Nothing has
  been committed or pushed by the agent; commits await owner instruction.
- `.zcode/plans/` exists in the repo for agent planning artifacts.
- Upstream remote issues are tracked at `glzr-io/glazewm` (this fork's issue
  knowledge lives in `.brain/worklog.md`).
