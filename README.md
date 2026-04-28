<div align="center">

# rtop

**A terminal-based system monitor for Windows, inspired by [btop](https://github.com/aristocratos/btop).**

Written in Rust. Fast. Beautiful. Zero dependencies at runtime.

[![Windows](https://img.shields.io/badge/platform-Windows%2011-0078D6?logo=windows)](https://github.com/freddiehaddad/rtop/releases)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

![rtop screenshot](docs/screenshot.png)

</div>

---

## What is rtop?

rtop is a resource monitor that shows CPU, memory, disk, network, GPU, and process information in a single terminal window. It's built from the ground up for **Windows** using native Win32 APIs — no WSL, no Linux compatibility layer, no MSYS.

The UI design is based on [btop](https://github.com/aristocratos/btop) by aristocratos, reimagined in Rust with Windows-native data collection and several enhancements:

- **Disk is a separate widget** — independently toggleable, not embedded in the memory panel
- **Preset system** — save/cycle/delete layout presets with `Ctrl+S` / `p` / `Ctrl+D`
- **GPU monitoring** via NVIDIA NVML — utilization, temperature, VRAM, power, clocks
- **CPU temperature** via LibreHardwareMonitor HTTP API
- **Per-box dirty rendering** — only redraws what changed, no full-screen flicker
- **40 bundled themes** — dracula, nord, gruvbox, tokyo-night, and more

## Features

| Widget | Data |
|--------|------|
| **CPU** | Per-core utilization, frequency, temperature, user/system graphs, load average |
| **Memory** | Used, available, cached, free, swap — with meter bars |
| **Disk** | Per-volume usage, filesystem type, capacity, read/write throughput, busy time |
| **Network** | Download/upload graphs with auto-scaling, interface selector |
| **GPU** | Utilization, temperature, VRAM, power draw (NVIDIA) |
| **Process** | PID, name, command line, CPU%, memory, tree view, filter, sort, terminate |

### Keybinds

| Key | Action |
|-----|--------|
| `m` / `Esc` | Toggle main menu |
| `h` / `F1` | Help |
| `o` / `F2` | Options |
| `1`–`6` | Toggle widgets (cpu/mem/net/proc/gpu/disk) |
| `p` / `P` | Cycle presets forward/back |
| `Ctrl+S` | Save current layout as preset |
| `+` / `-` | Adjust update speed |
| `f` / `/` | Filter processes |
| `e` | Tree view |
| `t` | Terminate process |
| `q` | Quit |

See the built-in help menu (`h`) for the complete list.

---

## Installation

### Download a release

1. Go to [Releases](https://github.com/freddiehaddad/rtop/releases)
2. Download the latest `rtop-x.y.z.zip`
3. Extract `rtop.exe` to a directory in your `PATH`
4. Run `rtop` from any terminal (Windows Terminal recommended)

### Build from source

**Requirements:**
- [Rust](https://rustup.rs/) (stable toolchain)
- Windows 10/11 SDK (included with Visual Studio Build Tools)

```powershell
git clone https://github.com/freddiehaddad/rtop.git
cd rtop
cargo build --release
```

The binary is at `target\release\rtop.exe`.

**Run tests:**

```powershell
cargo test
```

---

## Optional: Temperature Monitoring

CPU and GPU temperatures require [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) running in the background with its HTTP server enabled.

### Install via winget

```powershell
winget install LibreHardwareMonitor.LibreHardwareMonitor
```

### Enable the HTTP API

1. Open LibreHardwareMonitor
2. Go to **Options → Remote Web Server → Run**
3. The default port is `8085` — rtop reads from `http://localhost:8085`

### Start on boot (optional)

- In LibreHardwareMonitor: **Options → Start Minimized** and **Run On Windows Startup**

Without LibreHardwareMonitor, rtop works normally — temperature fields are simply blank.

---

## Optional: GPU Monitoring

NVIDIA GPU monitoring requires the NVIDIA driver to be installed (which provides `nvml.dll`). No additional software is needed. rtop detects NVIDIA GPUs automatically at runtime.

AMD and Intel GPU monitoring is not currently supported.

---

## Configuration

rtop stores its config at:

| Location | Path |
|----------|------|
| Config | `%APPDATA%\rtop\rtop.conf` (or `$XDG_CONFIG_HOME/rtop/`) |
| Logs | `%LOCALAPPDATA%\rtop\` (or `$XDG_STATE_HOME/rtop/`) |

The config file is created automatically on first run when `save_config_on_exit` is enabled (default). All options are editable from the built-in options menu (`o` / `F2`).

To print the default config:

```powershell
rtop --default-config
```

### Themes

rtop ships with 40 built-in themes. Change the theme from the options menu (General → color_theme) or set it in `rtop.conf`:

```
color_theme = "dracula"
```

---

## Command Line Options

```
Usage: rtop [OPTIONS]

Options:
  -c, --config <FILE>     Path to config file
  -d, --debug             Enable debug logging
  -f, --filter <TEXT>     Initial process filter
  -l, --low-color         Use 256-color mode
  -p, --preset <ID>       Start with a preset (0-9)
  -t, --tty               Force TTY mode
      --no-tty            Force disable TTY mode
  -u, --update <MS>       Update interval in milliseconds (min 100)
      --default-config    Print default config and exit
  -h, --help              Print help
  -V, --version           Print version
```

---

## Acknowledgments

- [btop](https://github.com/aristocratos/btop) by aristocratos — the original system monitor that inspired rtop's UI design
- [crossterm](https://github.com/crossterm-rs/crossterm) — terminal I/O
- [windows-rs](https://github.com/microsoft/windows-rs) — Windows API bindings
- [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) — hardware sensor access

---

## License

[MIT](LICENSE)
