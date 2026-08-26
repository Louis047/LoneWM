<div align="center">

# LoneWM

一个受 i3wm、bspwm 和 Hyprland 启发的高效、稳定的 Windows 平铺窗口管理器。

*在 AI 辅助下开发与维护。*

[![Downloads][downloads-badge]][downloads-link]
[![License: GPL-3.0][license-badge]][license-link]

[主要特性](#主要特性) •
[安装](#安装) •
[从源码构建](#从源码构建) •
[默认快捷键](#默认快捷键) •
[配置说明](#配置说明) •
[常见问题](#常见问题) •
[贡献 ↗](https://github.com/Louis047/LoneWM/blob/main/CONTRIBUTING.md)

![Demo video][demo-video]

</div>

---

## 主要特性

- **Dwindle 螺旋布局：** 新窗口自动将当前聚焦的窗口按交替螺旋方式分割，支持 2D 空间即时交换。
- **专注于 Windows 稳定性：** 修复了多项上游问题，涵盖窗口遮蔽（Cloaking）、UWP/Electron 挂起保留、多显示器睡眠唤醒重连、任务栏缩略图焦点处理以及全屏状态机转换。
- **低延迟输入：** 独立的低延迟键盘钩子线程（基于 `GetAsyncKeyState`）、哈希索引原生窗口查找以及源端 `WinEvent` 过滤。
- **Windows 11 视觉效果：** 原生彩色边框、亚克力/云母透明度、隐藏标题栏以及圆角控制。
- **Windows 原生架构：** 专为 Windows 打造，无跨平台抽象层开销。
- **多显示器支持：** 工作区显示器绑定、显示拓扑变更自适应以及 DPI 缩放支持。
- **WebSocket IPC：** 在 `127.0.0.1:6123` 上提供 JSON-over-WebSocket IPC 服务，便于脚本调用和第三方状态栏集成。

---

## 安装

**从 [GitHub Releases](https://github.com/Louis047/LoneWM/releases) 下载最新版本。**

* **安装程序 (`.exe`)：** 推荐使用，包含 UIAccess 权限以管理管理员权限（提权）运行的窗口。
* **便携版 (`.zip`)：** 包含独立可执行文件 (`lonewm.exe`、`lonewm-cli.exe`、`lonewm-watcher.exe`)。

---

## 从源码构建

LoneWM 使用 Rust **nightly** 工具链构建。在 Windows 上支持 GNU 工具链 (`mingw-w64`) 或 MSVC。

### 前置要求（GNU 工具链）
1. 安装用户级 Rust nightly：
   ```sh
   rustup default nightly-x86_64-pc-windows-gnu
   ```
2. 安装 `mingw-w64`（提供资源编译所需的 `windres.exe`）：
   ```sh
   scoop install mingw
   ```

### 构建命令
```sh
# 构建工作区的所有二进制文件 (lonewm, lonewm-cli, lonewm-watcher)
cargo build --workspace --release

# 运行测试
cargo test --workspace --tests
```

编译出的可执行文件位于 `target/release/`：
* `lonewm.exe`: 核心窗口管理器
* `lonewm-cli.exe`: CLI IPC 客户端
* `lonewm-watcher.exe`: 崩溃恢复监控守护进程

---

## 默认快捷键

在 LoneWM 首次启动时，会自动生成默认配置。

| 快捷键 | 命令 | 操作 |
| --- | --- | --- |
| `Alt + H` / `J` / `K` / `L` | `focus --direction left/down/up/right` | 朝指定方向切换焦点 |
| `Alt + Shift + H` / `J` / `K` / `L` | `move --direction left/down/up/right` | 朝指定方向与相邻窗口即时交换位置 |
| `Alt + 1` .. `9` | `focus --workspace 1..9` | 切换到指定工作区 |
| `Alt + Shift + 1` .. `9` | `move --workspace 1..9` + `focus` | 移动窗口到工作区并跟随焦点 |
| `Alt + V` | `toggle-tiling-direction` | 切换水平 $\leftrightarrow$ 垂直分割方向 |
| `Alt + T` | `toggle-tiling` | 将当前窗口设为平铺状态 |
| `Alt + Shift + Space` | `toggle-floating --centered` | 将当前窗口设为居中浮动状态 |
| `Alt + Space` | `wm-cycle-focus` | 轮转焦点（平铺 $\to$ 浮动 $\to$ 全屏） |
| `Alt + F` | `toggle-fullscreen --mode monocle` | 单片模式 1（填满工作区，保留任务栏与边距） |
| `Alt + Shift + F` | `toggle-fullscreen --mode full` | 全屏模式 0（覆盖整个显示器与任务栏） |
| `Alt + M` | `toggle-minimized` | 最小化 / 恢复当前聚焦窗口 |
| `Alt + Enter` | `shell-exec cmd` | 打开终端 |
| `Alt + Shift + Q` | `close` | 关闭当前聚焦窗口 |
| `Alt + Shift + P` | `wm-toggle-pause` | 暂停 / 恢复窗口管理器 |
| `Alt + Shift + E` | `wm-exit` | 安全退出 LoneWM |
| `Alt + Shift + R` | `wm-reload-config` | 重新加载配置文件 |
| `Alt + R` | `wm-enable-binding-mode --name resize` | 进入窗口调整大小模式 |

---

## 配置说明

### 配置文件查找顺序
LoneWM 按以下顺序查找配置文件：
1. `--config="..."` CLI 参数（例如 `lonewm.exe start --config="C:\path\to\config.yaml"`）
2. `LONEWM_CONFIG_PATH` 环境变量（支持兼容回退 `GLAZEWM_CONFIG_PATH`）
3. `%USERPROFILE%\.lonewm\config.yaml`
4. `%USERPROFILE%\.glzr\glazewm\config.yaml`（兼容旧路径）

如果启动时未找到配置文件，则会自动在 `%USERPROFILE%\.lonewm\config.yaml` 生成默认模板。

---

### 配置：常规 (General)

```yaml
general:
  # WM 启动时运行的命令。
  startup_commands: []

  # WM 退出前运行的命令。
  shutdown_commands: []

  # 重新加载配置后运行的命令。
  config_reload_commands: []

  # 是否自动聚焦鼠标光标下的窗口。
  focus_follows_cursor: true

  # 聚焦当前工作区时是否在上一工作区之间来回切换。
  toggle_workspace_on_refocus: false

  cursor_jump:
    # 切换焦点时是否自动移动鼠标光标。
    enabled: true
    # 'monitor_focus'（显示器间切换）或 'window_focus'（窗口间切换）。
    trigger: "monitor_focus"

  # 切换工作区时隐藏窗口的方式：
  # - 'cloak': 推荐的原生 DWM 遮蔽。
  # - 'hide': 传统的 ShowWindowAsync 隐藏。
  # - 'place_in_corner': 将窗口放置在屏幕可视区域外。
  hide_method: "cloak"

  # 任务栏按钮显示策略：
  # - true: 显示所有工作区的窗口。
  # - false: 仅显示当前活动工作区的窗口。
  show_all_in_taskbar: false
```

---

### 配置：边距与间隙 (Gaps)

内部与外部边距在所有方向上默认为相等的 **16px**。

```yaml
gaps:
  # 是否根据显示器 DPI 缩放边距。
  scale_with_dpi: true

  # 相邻窗口之间的间隙。
  inner_gap: "16px"

  # 窗口与屏幕边缘之间的间隙。
  outer_gap:
    top: "16px"
    right: "16px"
    bottom: "16px"
    left: "16px"
```

---

### 配置：工作区 (Workspaces)

LoneWM 采用 **纯粹的 Dwindle** 自动螺旋平铺布局。新窗口以交替方向（水平 $\to$ 垂直 $\to$ 水平）将当前聚焦窗口一分为二，向右下角级联分割。

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

### 配置：窗口效果 (Window Effects)

适用于 Windows 11 的视觉效果：

```yaml
window_effects:
  focused_window:
    # 聚焦窗口的彩色边框
    border:
      enabled: true
      color: "#8dbcff"

    # 隐藏标题栏
    hide_title_bar:
      enabled: false

    # 边角样式：'square'（直角）、'rounded'（圆角）、'small_rounded'（小圆角）
    corner_style:
      enabled: false
      style: "square"

    # 窗口透明度：'0%' 到 '100%'（例如 '95%'）
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

### 配置：窗口行为与双全屏模式 (Window Behavior)

LoneWM 借鉴了 Hyprland 的双全屏模式设计：

```yaml
window_behavior:
  initial_state: "tiling"
  state_defaults:
    floating:
      centered: true
      shown_on_top: false
    fullscreen:
      # 全屏模式：
      # - 'full' (模式 0): 覆盖整个物理显示器，忽略任务栏与窗口间隙。
      # - 'monocle' (模式 1): 填满工作区可用区域，保留任务栏与外部边距。
      mode: "full"
      respect_gaps: true
      shown_on_top: false
```

---

### 配置：窗口规则 (Window Rules)

根据进程名、标题或类名自动执行操作：

```yaml
window_rules:
  # 将浏览器自动移动到工作区 1
  - commands: ["move --workspace 1"]
    match:
      - window_process: { regex: "msedge|brave|chrome|zen" }

  # 忽略画中画小窗
  - commands: ["ignore"]
    match:
      - window_title: { regex: "[Pp]icture.in.[Pp]icture" }
        window_class: { regex: "Chrome_WidgetWin_1|MozillaDialogClass" }

  # 将特定程序对话框设为浮动
  - commands: ["set-floating"]
    match:
      - window_process: { equals: "Flow.Launcher" }
        window_title: { equals: "Settings" }
```

---

### 配置：快捷键参考 (Keybindings Reference)

快捷键使用 Windows 虚拟键码。Windows 徽标键请使用 `win` / `lwin` / `rwin`。

```yaml
keybindings:
  - commands: ["focus --workspace 1"]
    bindings: ["alt+1"]

  - commands: ["move --workspace 1", "focus --workspace 1"]
    bindings: ["alt+shift+1"]
```

<details>
<summary><b>支持的按键完整列表</b></summary>

| 按键 | 说明 |
| --- | --- |
| `a` - `z` | 字母键 |
| `0` - `9` | 数字键 |
| `numpad0` - `numpad9` | 小键盘数字键 |
| `f1` - `f24` | 功能键 |
| `shift`, `lshift`, `rshift` | Shift 键 |
| `control`, `lctrl`, `rctrl` | Control 键 |
| `alt`, `lalt`, `ralt` | Alt 键 |
| `lwin`, `rwin`, `win` | ⊞ Windows 键 |
| `space`, `enter`, `tab`, `escape`, `back` | 常用控制键 |
| `left`, `right`, `up`, `down` | 方向键 |
| `insert`, `delete`, `home`, `end`, `page_up`, `page_down` | 导航键 |
| `print_screen`, `scroll_lock`, `caps_lock`, `num_lock` | 锁定 / 实用键 |
| `oem_semicolon`, `oem_question`, `oem_tilde`, `oem_plus`, `oem_minus`, `oem_comma`, `oem_period` | OEM 标点符号键 |

</details>

---

## 常见问题

**问：如何开机自启动 LoneWM？**
右键单击系统托盘中的 LoneWM 图标，然后勾选 **"Run on system startup"**（开机启动）。

**问：LoneWM 中的平铺布局是如何工作的？**
LoneWM 采用 Dwindle 自动二叉树螺旋布局（灵感来自 `bspwm` 和 `Hyprland`）。每次打开新窗口都会以 50/50 的比例按深度交替方向分割当前聚焦的窗口。朝指定方向移动窗口时，会直接与屏幕上的相邻窗口即时交换位置。

**问：为什么以管理员权限运行的窗口没有被平铺？**
Windows 的用户界面特权隔离 (UIPI) 阻止普通权限进程移动、调整或聚焦提权窗口。官方签名的 LoneWM 安装程序包含 UIAccess 权限，可以直接管理提权窗口。对于未开启 UIAccess 的便携/本地构建版本，LoneWM 会自动忽略提权窗口以防止布局出现空位。若要在本地构建中管理提权窗口，请以管理员权限运行 LoneWM，或使用 `--features ui_access` 构建并对可执行文件进行数字签名。

**问：如何获取窗口的进程名、类名或标题以编写规则？**
可以使用 AutoHotkey Window Spy、Winlister 等工具，或者在窗口打开时在终端运行 `lonewm-cli query windows` 查询。

---

[downloads-badge]: https://img.shields.io/github/downloads/Louis047/LoneWM/total?logo=github&logoColor=white
[downloads-link]: https://github.com/Louis047/LoneWM/releases
[license-badge]: https://img.shields.io/badge/license-GPL--3.0-blue
[license-link]: https://github.com/Louis047/LoneWM/blob/main/LICENSE
[demo-video]: resources/assets/demo.webp

