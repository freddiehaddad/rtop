<div align="center">

# rtop

**A terminal-based system monitor for Windows, inspired by [btop](https://github.com/aristocratos/btop).**

Written in Rust. Fast. Beautiful.

[![Windows](https://img.shields.io/badge/platform-Windows%2011-0078D6?logo=windows)](https://github.com/freddiehaddad/rtop/releases)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPL--2.0-green)](LICENSE)

![rtop screenshot](docs/screenshot.png)

</div>

---

## What is rtop?

rtop is a resource monitor that shows CPU, memory, disk, network, GPU, and process information in a single terminal window. It's built from the ground up for **Windows** using native Win32 APIs.

The UI design is based on [btop](https://github.com/aristocratos/btop) by aristocratos, reimagined in Rust with Windows-native data collection and several enhancements:

- **Disk is a separate widget**: independently toggleable, not embedded in the memory panel
- **Preset system**: cycle through curated layout presets with `p` / `P`
- **Custom layouts**: beyond the built-in presets, arrange widgets into rows, columns, and nested panels with adjustable proportions ([details](#custom-layout))
- **GPU monitoring**: NVIDIA (NvAPI), AMD (ADL), and Intel (IGCL)
- **CPU temperature and power** via PawnIO kernel driver
- **Per-widget dirty rendering**: only redraws what changed
- **Per-widget refresh rates**: set individual update intervals for each widget
- **41 bundled themes**: dracula, nord, gruvbox, tokyo-night, and more
- **Vim key bindings**: optional h/j/k/l/g/G and Ctrl+F/B/D/U navigation
- **Process following**: pin a process with `F` to auto-scroll across refreshes
- **Statusbar**: borderless 1-row bar at the bottom with menu/preset/update interval (left) and uptime/clock (right)
- **Disk IO mode**: toggle between usage meters and throughput graphs

## Features

| Widget | Data |
|--------|------|
| **CPU** | Per-core utilization, frequency, temperature¹, power (watts)¹, user/system graphs, load average |
| **Memory** | Used, available, cached, free, swap — with meter bars |
| **Disk** | Per-volume usage, filesystem type, capacity, read/write throughput, busy time, IO mode |
| **Network** | Download/upload graphs with auto-scaling, interface selector, speed totals |
| **GPU** | Utilization, temperature, VRAM, power draw, clocks (NVIDIA, AMD, Intel) |
| **Process** | PID, name, command line, CPU%, memory, tree view, filter, sort, follow, terminate |

¹ Temperature and power readings require [PawnIO](#cpu-temperature--power) and Administrator privileges.

### Keybinds

#### Global

| Key | Action |
|-----|--------|
| `q` / `Ctrl+C` | Quit |
| `m` | Toggle main menu |
| `Esc` | Close detail panel |
| `?` / `F1` | Toggle help |
| `o` / `F2` | Toggle options |
| `p` / `Shift+P` | Cycle presets forward/back |
| `Ctrl+R` | Reload config |
| `+` / `-` | Adjust update speed |
| `1`–`6` | Toggle widget (cpu/mem/net/proc/disk/gpu) |
| `Shift+R` | Restore all hidden widgets |

#### Process

| Key | Action |
|-----|--------|
| `Up` / `Down` | Select process |
| `PgUp` / `PgDn` | Page through processes |
| `Home` / `End` | Jump to first/last |
| `Left` / `Right` | Cycle sort column |
| `r` | Toggle reverse sort |
| `f` / `/` | Filter processes |
| `e` | Toggle tree view |
| `c` | Toggle per-core CPU |
| `t` | Terminate process (graceful, double-tap) |
| `Shift+T` | Kill process (force, double-tap) |
| `Space` | Pause / resume process list (freeze the snapshot for inspection) |
| `Enter` | Show/hide process details |
| `Shift+F` | Follow/unfollow process |

#### Disk

| Key | Action |
|-----|--------|
| `i` | Toggle disk IO mode |

#### Network

| Key | Action |
|-----|--------|
| `n` / `b` | Cycle network interfaces |
| `a` | Toggle network auto scale |
| `s` | Toggle network sync scale |
| `z` | Reset network totals |

#### GPU

| Key | Action |
|-----|--------|
| `[` / `]` | Cycle GPU devices |


When **vim keys** are enabled (options → general):
`h`/`j`/`k`/`l` for directional control, `g`/`G` for top/bottom of list, `Ctrl+F`/`Ctrl+B` for page scrolling, `Ctrl+D`/`Ctrl+U` for half-page scrolling.

See the built-in help menu (`?` / `F1`) for the complete list.

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

## CPU Temperature & Power

CPU temperature and power readings require [PawnIO](https://pawnio.eu), a signed kernel driver that lets rtop read sensor data directly from the CPU. Supported on Intel and on AMD Zen and newer (Ryzen, Threadripper, EPYC). Without PawnIO, rtop runs normally and the temperature and power fields stay blank.

### Install via winget

```powershell
winget install --id namazso.PawnIO
```

After install, the PawnIO kernel driver and service are loaded automatically — no reboot required.

### Run rtop as Administrator

To see CPU temperature and power, launch rtop as Administrator (right-click `rtop.exe` → "Run as administrator", or open it from an elevated terminal). Without admin rights, rtop runs normally — the temperature and power rows just don't appear in the CPU widget.

---

## GPU Monitoring

rtop detects GPUs from all three major vendors automatically at runtime. For each detected device it reports **utilization, temperature, VRAM, power, and clocks**. Unlike CPU temperature and power, GPU monitoring needs no extra software beyond the vendor's graphics driver and no elevation.

| Vendor | Supported GPUs |
|--------|---------------|
| **NVIDIA** | GeForce 600 series (Kepler) and newer |
| **AMD** | Vega and newer (RX Vega, 5000, 6000, 7000, 9000 series) |
| **Intel** | Arc discrete GPUs |

Up to 8 GPUs are supported. Mixed GPU systems show all detected devices. If a vendor's driver is not installed, that backend is silently skipped.

---

## Configuration

rtop stores its config at:

| Location | Path |
|----------|------|
| Config | `%APPDATA%\rtop\rtop.toml` (or `$XDG_CONFIG_HOME/rtop/`) |
| Logs | `%LOCALAPPDATA%\rtop\` (or `$XDG_STATE_HOME/rtop/`) |

The config file is created automatically on first run and updated on every clean exit. All options are editable from the built-in options menu (`o` / `F2`).

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
cpu_update_ms = 1000     # CPU updates every 1s
proc_update_ms = 5000    # Processes update every 5s
mem_update_ms = 0        # 0 = use global (2000ms)
gpu_update_ms = 500      # GPU widget polls every 500ms
```

Set per-widget intervals via the options menu (each category tab has an update interval option) or in `rtop.toml`.

### Custom Layout

Want a layout that's different from the built-in presets? rtop lets you describe one with a small expression that combines widgets with vertical (`vstack`) and horizontal (`hstack`) panels. You choose both **which widgets are visible** and **how they're arranged**, including how wide each column should be.

The grammar:

```
layout := widget                                          # a single widget
        | vstack(layout, layout, ...)                     # children stacked top-to-bottom
        | hstack(weight:layout, weight:layout, ...)       # children laid out left-to-right
                                                          # weight is 1..255 (relative width share)
```

Widgets are `cpu`, `mem`, `net`, `proc`, `disk`, and `gpu`. Each widget appears at most once.

Edit the custom layout two ways:

1. **Options menu** (`o` or `F2`) → general → `custom_layout` — type a new expression and press Enter. Cycle to the `custom` preset (`p` / `P`) to see the result.
2. **`rtop.toml`** — set `preset = "custom"` and edit the `custom_layout` string at the top level.

#### Example 1 — minimal

CPU graph on top, processes filling the rest:

```toml
preset = "custom"
custom_layout = "vstack(cpu, proc)"
```

#### Example 2 — two-column dashboard with custom proportions

CPU spans the full width on top. Below, a left column stacks memory, network, and disk; the right column is the process list. The 40/60 weights give processes 60% of the width:

```toml
preset = "custom"
custom_layout = "vstack(cpu, hstack(40:vstack(mem, net, disk), 60:proc))"
```

### Hiding widgets at runtime

The number keys (`1`–`6` for cpu/mem/net/proc/disk/gpu) hide and restore individual widgets in the active layout. Hidden widgets stay hidden across preset cycles and across restarts (the filter is persisted in `rtop.toml`). Press `Shift+R` to restore everything in one keypress. The statusbar shows a `*` next to the preset name when any widget is hidden, so the filter never feels invisible.

Hide is purely a viewing operation — it never modifies your saved layout. The custom layout is only changed by the two paths above.

### Statusbar

The **statusbar** is a single-row bar that sits at the bottom of every preset. It keeps the things you reach for most often always visible, without taking space away from the widgets above it.

What it shows:

| Side | Item | Description |
|------|------|-------------|
| Left | Menu hint | `menu` keybind reminder |
| Left | Preset cycler | `← P NAME p →` — the active preset name; an asterisk after the name means at least one widget is hidden |
| Left | Update interval | `─ Nms +` — the global refresh rate |
| Right | Uptime | `up Xd HH:MM` — time since boot |
| Right | Clock | the current time, in your chosen format |

Every item can be turned on or off individually from the options menu (`o` / `F2` → Statusbar), and the whole bar can be hidden with one toggle if you want every row for widgets:

```toml
show_statusbar = true                  # master toggle
statusbar_show_menu = true
statusbar_show_preset = true
statusbar_show_update_interval = true
statusbar_show_uptime = true
statusbar_show_clock = true
statusbar_clock_format = "%T"          # see Clock format below
```

**Clock format.** The clock accepts a small subset of strftime specifiers — enough for any time-of-day layout you'd want in a status bar:

| Token | Meaning |
|-------|---------|
| `%T` | full time, `HH:MM:SS` |
| `%R` | short time, `HH:MM` |
| `%H` | hour, 24-hour (`00`–`23`) |
| `%I` | hour, 12-hour (`01`–`12`) |
| `%M` | minute (`00`–`59`) |
| `%S` | second (`00`–`59`) |
| `%p` | `AM` or `PM` |

Examples: `"%T"` → `13:24:05`, `"%I:%M %p"` → `01:24 PM`, `"%R"` → `13:24`. Set the format to an empty string to hide the clock without disabling the right section.

**Position is up to you.** The statusbar is a regular widget, so the [custom layout](#custom-layout) DSL can place `statusbar` anywhere — top, bottom, or beside another panel. The built-in presets put it at the bottom by default.

---

## Command Line Options

```
Usage: rtop [OPTIONS]

Options:
  -c, --config <FILE>     Path to config file
  -f, --filter <TEXT>     Initial process filter
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
- [PawnIO](https://pawnio.eu) by namazso — signed kernel driver for CPU MSR access
- [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) — `IntelMSR` and `AMDFamily17` bytecode modules embedded by rtop, licensed under LGPL-2.1-or-later

---

## License

[GPL-2.0-or-later](LICENSE)
