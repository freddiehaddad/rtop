<div align="center">

# rtop

**A terminal-based system monitor for Windows, inspired by [btop](https://github.com/aristocratos/btop).**

Written in Rust. Fast. Beautiful. Zero dependencies at runtime.

[![Windows](https://img.shields.io/badge/platform-Windows%2011-0078D6?logo=windows)](https://github.com/freddiehaddad/rtop/releases)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPL--2.0-green)](LICENSE)

![rtop screenshot](docs/screenshot.png)

</div>

---

## What is rtop?

rtop is a resource monitor that shows CPU, memory, disk, network, GPU, and process information in a single terminal window. It's built from the ground up for **Windows** using native Win32 APIs.

The UI design is based on [btop](https://github.com/aristocratos/btop) by aristocratos, reimagined in Rust with Windows-native data collection and several enhancements:

- **Disk is a separate widget** — independently toggleable, not embedded in the memory panel
- **Preset system** — save/cycle/delete layout presets with `Ctrl+S` / `p` / `Ctrl+X`
- **GPU monitoring** — NVIDIA (NvAPI), AMD (ADL), and Intel (IGCL) — utilization, temperature, VRAM, power, clocks
- **CPU temperature and power** via PawnIO kernel driver
- **Per-box dirty rendering** — only redraws what changed
- **Event-driven architecture** — per-collector threads with independent timers, channel-driven UI loop, zero CPU when idle
- **Per-widget update intervals** — each collector can run at its own speed
- **41 bundled themes** — dracula, nord, gruvbox, tokyo-night, and more
- **Vim key bindings** — optional h/j/k/l/g/G and Ctrl+F/B/D/U navigation
- **Process following** — pin a process with `F` to auto-scroll across refreshes
- **Clock display** — configurable clock in the CPU box
- **Disk IO mode** — toggle between usage meters and throughput graphs

## Features

| Widget | Data |
|--------|------|
| **CPU** | Per-core utilization, frequency, temperature, power (watts), user/system graphs, load average, uptime, clock |
| **Memory** | Used, available, cached, free, swap — with meter bars |
| **Disk** | Per-volume usage, filesystem type, capacity, read/write throughput, busy time, IO mode |
| **Network** | Download/upload graphs with auto-scaling, interface selector, speed totals |
| **GPU** | Utilization, temperature, VRAM, power draw, clocks (NVIDIA, AMD, Intel) |
| **Process** | PID, name, command line, CPU%, memory, tree view, filter, sort, follow, terminate |

### Keybinds

| Key | Action |
|-----|--------|
| `m` / `Esc` | Toggle main menu |
| `?` / `F1` | Help |
| `o` / `F2` | Options |
| `1`–`5` | Toggle widgets (cpu/mem/net/proc/disk) |
| `6`–`9` | Toggle GPU 0–3 |
| `0` | Toggle GPU 4–7 |
| `p` / `P` | Cycle presets forward/back |
| `Ctrl+S` | Save current layout as preset |
| `Ctrl+X` | Delete current preset |
| `Ctrl+R` | Reload config from file |
| `+` / `-` | Adjust update speed |
| `f` / `/` | Filter processes |
| `e` | Toggle tree view |
| `r` | Toggle reverse sort |
| `c` | Toggle per-core CPU |
| `i` | Toggle disk IO mode |
| `Enter` | Show/hide process details |
| `F` | Follow/unfollow process |
| `t` | Terminate process (graceful, double-tap) |
| `T` | Kill process (force, double-tap) |
| `n` / `b` | Cycle network interfaces |
| `a` | Toggle network auto scale |
| `y` | Toggle network sync scale |
| `z` | Reset network totals |
| `q` | Quit |

When **vim keys** are enabled (options → general):
`h`/`j`/`k`/`l` for directional control, `g`/`G` for top/bottom of list, `Ctrl+F`/`Ctrl+B` for page scrolling, `Ctrl+D`/`Ctrl+U` for half-page scrolling.

See the built-in help menu (`?`) for the complete list.

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

## Optional: Temperature & Power Monitoring

CPU temperature and power readings require [PawnIO](https://pawnio.eu) — a signed kernel driver that gives rtop direct access to CPU MSRs and AMD SMN registers without running a separate background application.

### Install via winget

```powershell
winget install namazso.PawnIO
```

### Run rtop as Administrator

Reading CPU MSRs requires kernel access, so rtop must be launched as Administrator (right-click → "Run as administrator"). Without elevation, rtop runs normally — temperature and power fields are simply blank.

rtop embeds the signed `IntelMSR` and `AMDFamily17` PawnIO modules from [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) (LGPL-2.1-or-later). At startup it detects the CPU vendor (Intel or AMD Family 17h+ / Zen and newer) and loads the matching module.

---

## GPU Monitoring

rtop detects GPUs from all three major vendors automatically at runtime. No additional software is needed beyond the vendor's graphics driver.

| Vendor | Supported GPUs | Metrics |
|--------|---------------|---------|
| **NVIDIA** | GeForce 600 series (Kepler) and newer | Utilization, temperature, VRAM, power, clocks |
| **AMD** | Vega and newer (RX Vega, 5000, 6000, 7000, 9000 series) | Utilization, temperature, VRAM, power, clocks |
| **Intel** | Arc discrete GPUs | Utilization, temperature, VRAM, power, clocks |

Up to 8 GPUs are supported. Mixed GPU systems show all detected devices. If a vendor's driver is not installed, that backend is silently skipped.

---

## Configuration

rtop stores its config at:

| Location | Path |
|----------|------|
| Config | `%APPDATA%\rtop\rtop.toml` (or `$XDG_CONFIG_HOME/rtop/`) |
| Logs | `%LOCALAPPDATA%\rtop\` (or `$XDG_STATE_HOME/rtop/`) |

The config file is created automatically on first run when `save_config_on_exit` is enabled (default). All options are editable from the built-in options menu (`o` / `F2`).

To print the default config:

```powershell
rtop --default-config
```

### Themes

rtop ships with 41 built-in themes. Change the theme from the options menu (General → Color Theme) or set it in `rtop.toml`:

```toml
color_theme = "dracula"
```

### Per-Widget Update Intervals

By default all widgets share the global `update_ms` interval (default 2000ms). Each widget can override this with its own interval:

```toml
update_ms = 2000
cpu_update_ms = 1000    # CPU updates every 1s
proc_update_ms = 5000   # Processes update every 5s
mem_update_ms = 0        # 0 = use global (2000ms)
```

Set per-widget intervals via the options menu (each category tab has an update interval option) or in `rtop.toml`.

### Visible Boxes

Control which widgets are shown via the `shown_boxes` config:

```toml
shown_boxes = ["cpu", "mem", "net", "proc", "disk"]
```

Toggle widgets at runtime with the `1`–`9` and `0` keys.

---

## Command Line Options

```
Usage: rtop [OPTIONS]

Options:
  -c, --config <FILE>     Path to config file
  -d, --debug             Enable debug logging
  -f, --filter <TEXT>     Initial process filter
  -p, --preset <ID>       Start with a preset (0-9)
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
- [PawnIO](https://pawnio.eu) by namazso — signed kernel driver for CPU MSR access; rtop embeds the LGPL-2.1-licensed `IntelMSR` and `AMDFamily17` modules from [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules)

---

## License

[GPL-2.0-or-later](LICENSE)
