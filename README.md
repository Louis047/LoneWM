<div align="center">

# LoneWM

A tiling window manager for Windows inspired by i3wm, bspwm, and Hyprland.

*Developed with AI assistance.*

[![Downloads][downloads-badge]][downloads-link]
[![License: GPL-3.0][license-badge]][license-link]

[Features](#features) •
[Installation](#installation) •
[Building from Source](#building-from-source) •
[Default Keybindings](#default-keybindings) •
[Configuration](#configuration) •
[FAQ](#faq) •
[Contributing ↗](https://github.com/Louis047/LoneWM/blob/main/CONTRIBUTING.md)

![Demo video][demo-video]

</div>

---

## Features

- **Dwindle Layout:** New windows split the currently focused window in an alternating spiral with spatial 2D window swapping.
- **Windows Stability:** Dedicated handling for cloaking, suspended UWP/Electron apps, multi-monitor sleep/wake events, taskbar focus handling, and fullscreen transitions.
- **Input & Event Handling:** Low-latency keyboard hook thread with `GetAsyncKeyState`, hash-indexed window lookups, and source-side event filtering.
- **Windows 11 Visual Effects:** Configurable window borders, acrylic/mica transparency, title bar toggle, and corner styling via DWM APIs.
- **Multi-Monitor Support:** Per-monitor workspace pinning, display change auto-reconciliation, and per-monitor DPI scaling.
- **IPC Server:** JSON-over-WebSocket IPC server on `127.0.0.1:6123` for scripting and third-party status bar integration.

---

## Installation

**Download the latest version from [GitHub Releases](https://github.com/Louis047/LoneWM/releases).**

* **Installer (`.exe`):** Recommended for full features. Includes UIAccess permissions to manage elevated/administrator windows.
* **Portable (`.zip`):** Self-contained binaries (`lonewm.exe`, `lonewm-cli.exe`, `lonewm-watcher.exe`).

---

## Building from Source

LoneWM uses the Rust **nightly** toolchain. On Windows, it builds with either GNU toolchain (`mingw-w64`) or MSVC.

### Prerequisites (GNU toolchain)
1. Install user-scoped Rust nightly:
   ```sh
   rustup default nightly-x86_64-pc-windows-gnu
   ```
2. Install `mingw-w64` (provides `windres.exe` for resource compilation):
   ```sh
   scoop install mingw
   ```

### Build Commands
```sh
# Build all workspace binaries (lonewm, lonewm-cli, lonewm-watcher)
cargo build --workspace --release

# Run tests
cargo test --workspace --tests
```

Built executables are located in `target/release/`:
* `lonewm.exe`: Core window manager
* `lonewm-cli.exe`: CLI IPC client
* `lonewm-watcher.exe`: Crash-recovery watchdog

---

## Default Keybindings

| Shortcut | Command | Action |
| --- | --- | --- |
| `Alt + H` / `J` / `K` / `L` | `focus --direction left/down/up/right` | Shift focus in direction |
| `Alt + Shift + H` / `J` / `K` / `L` | `move --direction left/down/up/right` | Swap focused window with neighbor in direction |
| `Alt + 1` .. `9` | `focus --workspace 1..9` | Switch to workspace |
| `Alt + Shift + 1` .. `9` | `move --workspace 1..9` + `focus` | Move window to workspace & follow |
| `Alt + V` | `toggle-tiling-direction` | Toggle horizontal $\leftrightarrow$ vertical split |
| `Alt + T` | `toggle-tiling` | Set focused window to tiling |
| `Alt + Shift + Space` | `toggle-floating --centered` | Set focused window to centered floating |
| `Alt + Space` | `wm-cycle-focus` | Cycle focus (Tiling $\to$ Floating $\to$ Fullscreen) |
| `Alt + F` | `toggle-fullscreen --mode monocle` | Monocle Mode 1 (fills workspace, keeping taskbar & gaps) |
| `Alt + Shift + F` | `toggle-fullscreen --mode full` | Fullscreen Mode 0 (covers entire monitor & taskbar) |
| `Alt + M` | `toggle-minimized` | Minimize / restore focused window |
| `Alt + Enter` | `shell-exec cmd` | Launch terminal |
| `Alt + Shift + Q` | `close` | Close focused window |
| `Alt + Shift + P` | `wm-toggle-pause` | Pause / unpause window management |
| `Alt + Shift + E` | `wm-exit` | Exit LoneWM safely |
| `Alt + Shift + R` | `wm-reload-config` | Reload configuration file |
| `Alt + R` | `wm-enable-binding-mode --name resize` | Enter window resize mode |

---

## Configuration

### Configuration File Locations
LoneWM discovers its configuration in the following order:
1. `--config="..."` CLI argument (e.g. `lonewm.exe start --config="C:\path\to\config.yaml"`)
2. `LONEWM_CONFIG_PATH` environment variable (legacy `GLAZEWM_CONFIG_PATH` fallback)
3. `%USERPROFILE%\.lonewm\config.yaml`
4. `%USERPROFILE%\.glzr\glazewm\config.yaml` (legacy fallback)

If no config is found on startup, a default template is written to `%USERPROFILE%\.lonewm\config.yaml`.

---

### Config: General

```yaml
general:
  # Commands to run when the WM has started.
  startup_commands: []

  # Commands to run just before the WM shuts down.
  shutdown_commands: []

  # Commands to run after the WM config has reloaded.
  config_reload_commands: []

  # Whether to automatically focus windows underneath the cursor.
  focus_follows_cursor: true

  # Switch back and forth between previous workspace when focusing current workspace.
  toggle_workspace_on_refocus: false

  cursor_jump:
    # Whether to automatically move cursor on focus change.
    enabled: true
    # 'monitor_focus' (between monitors) or 'window_focus' (between windows).
    trigger: "monitor_focus"

  # How windows are hidden when switching workspaces:
  # - 'cloak': Recommended native DWM cloaking.
  # - 'hide': Legacy ShowWindowAsync hide.
  # - 'place_in_corner': Positions windows off-screen.
  hide_method: "cloak"

  # Affects taskbar buttons:
  # - true: Show windows from all workspaces.
  # - false: Only show windows from active workspaces.
  show_all_in_taskbar: false
```

---

### Config: Gaps

Inner and outer gaps default to an equal **16px** on all sides.

```yaml
gaps:
  # Whether to scale gaps with monitor DPI.
  scale_with_dpi: true

  # Gap between adjacent windows.
  inner_gap: "16px"

  # Gap between windows and screen edges.
  outer_gap:
    top: "16px"
    right: "16px"
    bottom: "16px"
    left: "16px"
```

---

### Config: Workspaces

LoneWM uses a **pure Dwindle** automatic spiral tiling layout. New windows split the currently focused window in an alternating spiral (horizontal $\to$ vertical $\to$ horizontal), cascading toward the bottom-right.

```yaml
workspaces:
  - name: "1"
    display_name: "Web"
    bind_to_monitor: 0
    keep_alive: false

  - name: "2"
    display_name: "Code"

  - name: "3"
  - name: "4"
  - name: "5"
```

---

### Config: Window Effects

Visual effects exclusive to Windows 11:

```yaml
window_effects:
  focused_window:
    # Colored border around focused window
    border:
      enabled: true
      color: "#8dbcff"

    # Remove window title bar
    hide_title_bar:
      enabled: false

    # Corner style: 'square', 'rounded', 'small_rounded'
    corner_style:
      enabled: false
      style: "square"

    # Window transparency: '0%' to '100%' (e.g. '95%')
    transparency:
      enabled: false
      opacity: "95%"

  other_windows:
    border:
      enabled: true
      color: "#a1a1a1"
    hide_title_bar:
      enabled: false
    corner_style:
      enabled: false
      style: "square"
    transparency:
      enabled: false
      opacity: "0%"
```

---

### Config: Window Behavior & Dual Fullscreen Modes

LoneWM supports Hyprland-style dual fullscreen modes:

```yaml
window_behavior:
  initial_state: "tiling"
  state_defaults:
    floating:
      centered: true
      shown_on_top: false
    fullscreen:
      # Fullscreen mode:
      # - 'full' (Mode 0): Covers the entire monitor screen, hiding taskbars and ignoring gaps.
      # - 'monocle' (Mode 1): Expands to fill the workspace work area, keeping taskbar and gaps visible.
      mode: "full"
      respect_gaps: true
      shown_on_top: false
```

---

### Config: Window Rules

Automate window behavior based on process name, title, or window class:

```yaml
window_rules:
  # Move browsers to workspace 1
  - commands: ["move --workspace 1"]
    match:
      - window_process: { regex: "msedge|brave|chrome|zen" }

  # Ignore picture-in-picture windows
  - commands: ["ignore"]
    match:
      - window_title: { regex: "[Pp]icture.in.[Pp]icture" }
        window_class: { regex: "Chrome_WidgetWin_1|MozillaDialogClass" }

  # Float specific application dialogs
  - commands: ["set-floating"]
    match:
      - window_process: { equals: "Flow.Launcher" }
        window_title: { equals: "Settings" }
```

---

### Config: Keybindings Reference

Keybindings use Windows virtual keys. Use `win`/`lwin`/`rwin` for Windows logo keys.

```yaml
keybindings:
  - commands: ["focus --workspace 1"]
    bindings: ["alt+1"]

  - commands: ["move --workspace 1", "focus --workspace 1"]
    bindings: ["alt+shift+1"]
```

<details>
<summary><b>Full list of supported keys</b></summary>

| Key | Description |
| --- | --- |
| `a` - `z` | Letter keys |
| `0` - `9` | Number keys |
| `numpad0` - `numpad9` | Numpad digits |
| `f1` - `f24` | Function keys |
| `shift`, `lshift`, `rshift` | Shift keys |
| `control`, `lctrl`, `rctrl` | Control keys |
| `alt`, `lalt`, `ralt` | Alt keys |
| `lwin`, `rwin`, `win` | ⊞ Windows keys |
| `space`, `enter`, `tab`, `escape`, `back` | Common control keys |
| `left`, `right`, `up`, `down` | Arrow keys |
| `insert`, `delete`, `home`, `end`, `page_up`, `page_down` | Navigation keys |
| `print_screen`, `scroll_lock`, `caps_lock`, `num_lock` | Lock / utility keys |
| `oem_semicolon`, `oem_question`, `oem_tilde`, `oem_plus`, `oem_minus`, `oem_comma`, `oem_period` | OEM punctuation keys |

</details>

---

## FAQ

**Q: How do I run LoneWM on startup?**
Right-click the LoneWM system tray icon and check **"Run on system startup"**.

**Q: How do layouts work in LoneWM?**
LoneWM uses a dwindle binary tree layout (inspired by `bspwm` and `Hyprland`). Every new window splits the currently focused window 50/50 in an alternating direction (depth-parity spiral). Moving windows in a direction swaps leaf positions with the adjacent window on screen.

**Q: Why are windows running as Administrator (elevated) not tiled?**
Windows UIPI blocks standard-integrity processes from repositioning or focusing elevated windows. The official signed LoneWM installer runs with UIAccess to manage elevated windows. For local/portable builds without UIAccess, LoneWM automatically ignores elevated windows to avoid phantom tiling slots. To manage elevated windows with a local build, run LoneWM as Administrator or build with `--features ui_access` and sign the binary.

**Q: How do I inspect a window's process name, class, or title for rules?**
Use tools like AutoHotkey Window Spy or Winlister, or run `lonewm-cli query windows` in a terminal while the window is open.

---

[downloads-badge]: https://img.shields.io/github/downloads/Louis047/LoneWM/total?logo=github&logoColor=white
[downloads-link]: https://github.com/Louis047/LoneWM/releases
[license-badge]: https://img.shields.io/badge/license-GPL--3.0-blue
[license-link]: https://github.com/Louis047/LoneWM/blob/main/LICENSE
[demo-video]: resources/assets/demo.webp
