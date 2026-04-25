# rtop — Complete Implementation Plan

## A full Rust rewrite of btop for Windows 11, using Test-Driven Development

---

## 1. Project Overview

**rtop** is a terminal-based system resource monitor written in Rust, targeting Windows 11 exclusively. It aims for complete feature parity with [btop](https://github.com/aristocratos/btop) v1.4.6, the Linux C++ system monitor, including all UI elements, keybinds, themes, configuration options, menus, graphs, and process management features.

### Guiding Principles

- **Pure idiomatic Rust** — leverage the type system, ownership model, and ecosystem
- **Test-driven development** — tests are written before implementation for every module
- **Off-screen cell buffer** — all rendering targets an internal buffer for deterministic snapshot testing
- **Graceful degradation** — features that cannot map to Windows degrade with clear UI feedback
- **Zero unsafe where possible** — prefer safe abstractions; `unsafe` only for FFI boundaries

### Architecture Summary

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                              │
│              (startup, event loop, shutdown)                │
├─────────────────────────────────────────────────────────────┤
│                      runner (async)                         │
│         (collection thread + draw coordinator)              │
├──────────┬──────────┬──────────┬──────────┬─────────────────┤
│ ui::cpu  │ ui::mem  │ ui::net  │ ui::proc │ ui::gpu (opt)   │
│          │          │          │          │                 │
│  Box renderers — write to CellBuffer                        │
├──────────┴──────────┴──────────┴──────────┴─────────────────┤
│                     draw (primitives)                       │
│   Graph · Meter · TextEdit · createBox · calcSizes          │
├─────────────────────────────────────────────────────────────┤
│                   cell_buffer (off-screen)                  │
│              Cell { ch, fg, bg, attrs } grid                │
├─────────────────────────────────────────────────────────────┤
│                    menu (overlays)                          │
│   Main · Options · Help · SignalChoose · Renice · msgBox    │
├──────────┬──────────┬──────────┬──────────┬─────────────────┤
│collect:: │collect:: │collect:: │collect:: │collect::gpu     │
│  cpu     │  mem     │  net     │  proc    │  (optional)     │
├──────────┴──────────┴──────────┴──────────┴─────────────────┤
│                     domain models                           │
│  CpuInfo · MemInfo · NetInfo · ProcInfo · GpuInfo           │
│  DiskInfo · BatteryInfo · Capability flags                  │
├─────────────────────────────────────────────────────────────┤
│  config  │  theme   │  tools   │   log    │   input         │
├──────────┴──────────┴──────────┴──────────┴─────────────────┤
│                   term (backend)                            │
│        crossterm + Windows Console API fallbacks            │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Windows Capability Matrix

Before implementing anything, every btop feature must be mapped to its Windows equivalent. Features are classified as: **✅ Supported**, **🔄 Emulated** (different API, same result), **⚠️ Degraded** (partial), or **❌ Unsupported** (omitted with UI notice).

### 2.1 CPU Metrics

| btop Feature | Windows API | Status |
|---|---|---|
| Total CPU % | `PDH \Processor(_Total)\% Processor Time` or `NtQuerySystemInformation(SystemProcessorPerformanceInformation)` | ✅ |
| Per-core CPU % | `PDH \Processor(N)\% Processor Time` or per-processor `SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION` | ✅ |
| CPU model name | `CPUID` instruction / Registry `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\ProcessorNameString` | ✅ |
| CPU frequency | `CallNtPowerInformation(ProcessorInformation)` / `NtQuerySystemInformation(SystemProcessorPerformanceInformation)` for per-core current freq | ✅ |
| CPU temperature | WMI `MSAcpi_ThermalZoneTemperature` (requires admin) or LibreHardwareMonitor WMI bridge | ⚠️ Degraded — requires admin or third-party service |
| Per-core temperature | LibreHardwareMonitor WMI bridge (`/namespace:root/LibreHardwareMonitor`) | ⚠️ Degraded — requires third-party service |
| Load average (1/5/15 min) | No direct equivalent. Emulate by computing rolling CPU avg over 1/5/15 min windows | 🔄 Emulated |
| CPU power (watts) | No standard API. LibreHardwareMonitor if available, else omit | ⚠️ Degraded |
| Battery status | `GetSystemPowerStatus()` — AC/battery, %, time remaining | ✅ |
| Battery watts | `IOCTL_BATTERY_QUERY_STATUS` via `SetupDi*` + DeviceIoControl for rate | ⚠️ Degraded — requires device enumeration |
| System uptime | `GetTickCount64()` | ✅ |
| CPU user/nice/system/idle/iowait/irq/softirq/steal/guest breakdown | Windows only exposes Idle/Kernel/User/DPC/Interrupt time. Map: `user`→UserTime, `system`→KernelTime-IdleTime, `idle`→IdleTime, `irq`→InterruptTime, `dpc`→DpcTime. Others show as 0 | 🔄 Emulated — 5 of 11 fields populated |

### 2.2 Memory Metrics

| btop Feature | Windows API | Status |
|---|---|---|
| Total/used/free/available/cached RAM | `GlobalMemoryStatusEx()` for total/available. `GetPerformanceInfo()` for cache. Used = total - available | ✅ |
| Swap total/used/free | `GlobalMemoryStatusEx()` → `ullTotalPageFile`, `ullAvailPageFile`. Swap = PageFile - PhysicalMem | ✅ |
| Disk total/used/free per mount | `GetLogicalDriveStrings()` + `GetDiskFreeSpaceExW()` per drive letter | ✅ |
| Disk filesystem type | `GetVolumeInformationW()` → fstype (NTFS, FAT32, ReFS, etc.) | ✅ |
| Disk IO read/write bytes | PDH `\PhysicalDisk(*)\Disk Read Bytes/sec` and `Disk Write Bytes/sec` | ✅ |
| Disk IO activity % | PDH `\PhysicalDisk(*)\% Disk Time` | ✅ |
| ZFS ARC cache | Not applicable on Windows | ❌ Omit silently |

### 2.3 Network Metrics

| btop Feature | Windows API | Status |
|---|---|---|
| Interface enumeration | `GetAdaptersAddresses()` with `GAA_FLAG_INCLUDE_ALL_INTERFACES` | ✅ |
| Per-interface RX/TX bytes | `GetIfEntry2()` → `InOctets`, `OutOctets` | ✅ |
| Download/upload speed | Delta RX/TX bytes over delta time | ✅ |
| IPv4/IPv6 addresses | `GetAdaptersAddresses()` → `FirstUnicastAddress` | ✅ |
| Interface connected status | `MIB_IF_ROW2.MediaConnectState` | ✅ |
| MAC address | `GetAdaptersAddresses()` → `PhysicalAddress` | ✅ |

### 2.4 Process Metrics

| btop Feature | Windows API | Status |
|---|---|---|
| Process enumeration (PID, name) | `CreateToolhelp32Snapshot()` + `Process32First/Next` | ✅ |
| Process CPU % | `GetProcessTimes()` → UserTime + KernelTime deltas | ✅ |
| Process memory (working set) | `GetProcessMemoryInfo()` → `WorkingSetSize` | ✅ |
| Process command line | `NtQueryInformationProcess` → `ProcessBasicInformation` → PEB → `ProcessParameters.CommandLine` (or WMI `Win32_Process.CommandLine`) | ⚠️ Requires SeDebugPrivilege for some processes |
| Process username | `OpenProcessToken()` + `GetTokenInformation(TokenUser)` + `LookupAccountSidW()` | ✅ |
| Process parent PID | `PROCESSENTRY32.th32ParentProcessID` | ✅ |
| Process thread count | `PROCESSENTRY32.cntThreads` | ✅ |
| Process state | `WaitForSingleObject(hProcess, 0)` + `IsProcessInJob` + `NtQueryInformationProcess(ProcessBasicInformation)` → exitcode check. Map to: Running, Suspended, Not Responding | 🔄 Emulated — fewer states than Linux |
| Process nice/priority | `GetPriorityClass()` → IDLE/BELOW_NORMAL/NORMAL/ABOVE_NORMAL/HIGH/REALTIME | 🔄 6 classes instead of -20..19 |
| Process start time | `GetProcessTimes()` → `CreationTime` | ✅ |
| Process IO read/write | `GetProcessIoCounters()` → `ReadTransferCount`, `WriteTransferCount` | ✅ |
| Process tree (parent-child) | Build from PPID relationships via `CreateToolhelp32Snapshot` | ✅ |
| Send signal (SIGTERM/SIGKILL/etc.) | `TerminateProcess()` for kill. No signal equivalent — menu shows: Terminate, End Task (WM_CLOSE via `EnumWindows`), Suspend (`NtSuspendProcess`), Resume (`NtResumeProcess`) | 🔄 Emulated — 4 actions replace 31 signals |
| Renice (change priority) | `SetPriorityClass()` with 6 priority classes | 🔄 Emulated |
| Process filtering (regex) | Pure Rust regex on name/cmd — same behavior | ✅ |
| Kernel thread filtering | Filter by `Session == 0` and known system processes | 🔄 Emulated |

### 2.5 GPU Metrics

| btop Feature | Windows API | Status |
|---|---|---|
| NVIDIA GPU metrics | NVML (`nvml.dll` — ships with driver on Windows) | ✅ |
| AMD GPU metrics | ADLX SDK (amd_ags library) or WMI `Win32_VideoController` | ⚠️ ADLX availability varies |
| Intel GPU metrics | `D3DKMT` (DirectX Kernel Mode) — basic util only | ⚠️ Limited metrics |
| GPU utilization % | NVML: `nvmlDeviceGetUtilizationRates`. D3DKMT for others | ✅/⚠️ |
| GPU temperature | NVML: `nvmlDeviceGetTemperature`. AMD: ADLX. Intel: not available | ✅/⚠️/❌ |
| GPU VRAM total/used | NVML: `nvmlDeviceGetMemoryInfo`. D3DKMT: `D3DKMTQueryStatistics` | ✅ |
| GPU power (watts) | NVML: `nvmlDeviceGetPowerUsage`. AMD: ADLX. Intel: N/A | ✅/⚠️/❌ |
| GPU clock speeds | NVML: `nvmlDeviceGetClockInfo`. AMD: ADLX | ✅/⚠️ |
| GPU encoder/decoder | NVML: `nvmlDeviceGetEncoderUtilization` | ✅ NVIDIA only |
| GPU PCIe throughput | NVML: `nvmlDeviceGetPcieThroughput` | ✅ NVIDIA only |

### 2.6 UI/Terminal

| btop Feature | Windows API | Status |
|---|---|---|
| Alternate screen buffer | VT sequence `\x1b[?1049h` (Windows Terminal supports it) | ✅ |
| 24-bit truecolor | VT sequence `\x1b[38;2;R;G;Bm` — Windows Terminal supports it | ✅ |
| 256-color fallback | VT sequence `\x1b[38;5;Nm` | ✅ |
| Mouse tracking (click, drag, scroll) | VT mouse sequences (SGR mode `\x1b[?1006h`) or `ReadConsoleInput` for legacy | ✅ |
| Terminal resize detection | `crossterm` resize event or `SetConsoleCtrlHandler` + `GetConsoleScreenBufferInfo` | ✅ |
| Braille characters | Requires font support (Cascadia Code, etc.) — degrade to block/tty if not | ✅ (font-dependent) |
| Box-drawing characters | Universal support on Windows Terminal | ✅ |
| Synchronized output | `\x1b[?2026h/l` — Windows Terminal 1.16+ | ✅ |
| Ctrl+Z (suspend) | Not applicable on Windows — remap or omit | ❌ Omit |
| SUID privilege management | Not applicable on Windows — use "Run as Administrator" semantics | ❌ N/A |

### 2.7 Configuration

| btop Feature | Windows Mapping | Status |
|---|---|---|
| Config file location | `%APPDATA%\rtop\rtop.conf` | ✅ |
| Theme file location | `%APPDATA%\rtop\themes\` + bundled themes | ✅ |
| Log file location | `%LOCALAPPDATA%\rtop\rtop.log` | ✅ |
| `/etc/fstab`, `/etc/mtab` configs | Not applicable — `use_fstab` and `zfs_*` configs hidden | ❌ N/A |
| `freq_mode` (first/range/avg) | Supported via per-core frequency query | ✅ |
| `proc_filter_kernel` | Filter system/session-0 processes | 🔄 Emulated |
| `show_cpu_watts` | Only if LibreHardwareMonitor available | ⚠️ |

---

## 3. Crate Selection

### 3.1 Core Dependencies

| Crate | Purpose | Justification |
|---|---|---|
| `crossterm` | Terminal I/O, raw mode, mouse, resize, colors, alternate screen | Best cross-platform terminal crate; VT-first with Windows Console fallback. Avoids reimplementing escape sequences |
| `windows` (microsoft/windows-rs) | Win32 API bindings for collectors | Official Microsoft crate; zero-cost FFI; fine-grained feature flags |
| `clap` (derive) | CLI argument parsing | Industry standard; derive macro for zero-boilerplate argument structs |
| `regex` | Process filtering | btop supports regex filters with `!` prefix |
| `unicode-width` | Character display width | Required for accurate column alignment with CJK/emoji |
| `unicode-segmentation` | Grapheme cluster iteration | Required for cursor movement in TextEdit |
| `parking_lot` | Mutex/RwLock | Faster than std, no poisoning |
| `tracing` + `tracing-appender` | Structured logging with file rotation | More capable than hand-rolled logger; supports levels, file rotation, async writes |
| `directories` | Platform-appropriate config/data/log paths | `%APPDATA%`, `%LOCALAPPDATA%` resolution |
| `serde` + `toml`/custom parser | Config serialization | For config file I/O (btop uses custom format; we replicate it) |
| `once_cell` or `std::sync::LazyLock` | Lazy statics | Thread-safe lazy initialization |

### 3.2 Optional/Dev Dependencies

| Crate | Purpose |
|---|---|
| `insta` | Snapshot testing for rendered output |
| `proptest` | Property-based testing for layout calculations |
| `criterion` | Benchmarking for rendering hot paths |
| `mockall` | Trait mocking for collector tests |
| `pretty_assertions` | Better assertion diff output |

---

## 4. Module Structure

```
src/
├── main.rs                    # Entry point, startup, event loop, shutdown
├── cli.rs                     # Command-line argument parsing
├── config.rs                  # Configuration loading/saving/defaults
├── theme.rs                   # Theme file parsing, color conversion, gradients
├── log.rs                     # Logging initialization and setup
├── tools.rs                   # String utilities, formatting, time helpers
├── term.rs                    # Terminal abstraction (crossterm wrapper)
├── input.rs                   # Input event translation and routing
├── cell_buffer.rs             # Off-screen rendering buffer (Cell grid)
├── draw/
│   ├── mod.rs                 # Re-exports
│   ├── graph.rs               # Graph class (braille/block/tty)
│   ├── meter.rs               # Meter class (progress bars)
│   ├── text_edit.rs           # TextEdit class (input field)
│   ├── box_drawing.rs         # createBox, border characters, symbols
│   └── layout.rs              # calcSizes, dynamic box sizing/reflow
├── domain/
│   ├── mod.rs                 # Re-exports
│   ├── cpu.rs                 # CpuInfo, CpuSample, CoreSample
│   ├── memory.rs              # MemInfo, DiskInfo, SwapInfo
│   ├── network.rs             # NetInfo, NetStat, InterfaceInfo
│   ├── process.rs             # ProcInfo, DetailContainer, TreeProc
│   └── gpu.rs                 # GpuInfo, GpuSupported
├── collect/
│   ├── mod.rs                 # Collector trait, shared logic
│   ├── cpu.rs                 # Windows CPU collector
│   ├── memory.rs              # Windows memory/disk collector
│   ├── network.rs             # Windows network collector
│   ├── process.rs             # Windows process collector
│   └── gpu.rs                 # GPU collector (NVML/ADLX/D3DKMT)
├── ui/
│   ├── mod.rs                 # Box trait, re-exports
│   ├── cpu_box.rs             # CPU box renderer
│   ├── mem_box.rs             # Memory/disk box renderer
│   ├── net_box.rs             # Network box renderer
│   ├── proc_box.rs            # Process box renderer
│   └── gpu_box.rs             # GPU box renderer
├── menu/
│   ├── mod.rs                 # Menu state machine, re-exports
│   ├── main_menu.rs           # ASCII art logo menu
│   ├── options_menu.rs        # Options with 5+ categories
│   ├── help_menu.rs           # Keybind reference
│   ├── signal_menu.rs         # Process actions (terminate/suspend/resume)
│   ├── priority_menu.rs       # Priority class selection
│   └── msg_box.rs             # Modal dialog (OK / Yes-No)
├── runner.rs                  # Background collection/render thread
└── banner.rs                  # ASCII art banner with colors
```

### Theme and config files bundled at compile time:

```
themes/                        # All 40 .theme files from btop
  dracula.theme
  nord.theme
  gruvbox_dark.theme
  ... (40 total)
```

---

## 5. Phased Implementation Plan (TDD)

Each phase follows the Red-Green-Refactor cycle:
1. **Red** — Write failing tests that specify the expected behavior
2. **Green** — Write the minimum code to make tests pass
3. **Refactor** — Clean up while keeping tests green

---

### Phase 0: Project Scaffolding & Capability Spec

**Goal:** Set up project structure, CI, and the Windows capability decisions documented in §2.

#### Tasks

- [ ] Initialize Cargo workspace structure with all module stubs
- [ ] Configure `Cargo.toml` with all dependencies and feature flags
- [ ] Set up `cargo test`, `cargo clippy`, `cargo fmt` in CI
- [ ] Create feature flag `gpu` for optional GPU support
- [ ] Create `domain/` module with all data type stubs
- [ ] Document all §2 capability decisions as code comments in domain types

#### Tests (Phase 0)

- Cargo compiles with no errors
- All module stubs are importable
- Feature flag `gpu` toggles GPU module inclusion

---

### Phase 1: Core Domain Types & App State

**Goal:** Define every data structure that flows between collectors and UI. These are the canonical types — collectors populate them, UI reads them.

#### 1.1 CPU Domain (`domain/cpu.rs`)

```rust
pub struct CpuInfo {
    pub cpu_percent: HashMap<String, VecDeque<i64>>,  // "total","user","system","idle","irq","dpc"
    pub core_percent: Vec<VecDeque<i64>>,              // Per-core usage history
    pub temp: Vec<VecDeque<i64>>,                      // Per-core temperature history (°C)
    pub temp_max: i64,                                 // Critical temperature threshold
    pub load_avg: [f64; 3],                            // Emulated 1/5/15 min rolling average
    pub usage_watts: f32,                              // CPU power consumption (0.0 if unavailable)
    pub cpu_name: String,                              // Model name
    pub cpu_hz: String,                                // Current frequency string
    pub core_count: usize,
    pub has_battery: bool,
    pub battery: BatteryInfo,
    pub uptime_seconds: u64,
}

pub struct BatteryInfo {
    pub percent: i32,                   // 0-100, -1 if unavailable
    pub watts: f32,                     // Discharge/charge rate
    pub seconds_remaining: i64,         // -1 if unknown
    pub status: String,                 // "Charging", "Discharging", "Full", "No Battery"
    pub ac_connected: bool,
}
```

#### 1.2 Memory Domain (`domain/memory.rs`)

```rust
pub struct MemInfo {
    pub stats: HashMap<String, u64>,              // "used","available","cached","free","swap_total","swap_used","swap_free"
    pub percent: HashMap<String, VecDeque<i64>>,  // Same keys, percentage histories
    pub disks: IndexMap<String, DiskInfo>,         // Ordered by drive letter
    pub disks_order: Vec<String>,
}

pub struct DiskInfo {
    pub name: String,            // "C:", "D:", etc.
    pub label: String,           // Volume label
    pub fstype: String,          // "NTFS", "FAT32", "ReFS"
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub used_percent: i32,
    pub free_percent: i32,
    pub io_read: VecDeque<i64>,
    pub io_write: VecDeque<i64>,
    pub io_activity: VecDeque<i64>,
}
```

#### 1.3 Network Domain (`domain/network.rs`)

```rust
pub struct NetInfo {
    pub bandwidth: HashMap<String, VecDeque<i64>>,  // "download", "upload"
    pub stat: HashMap<String, NetStat>,
    pub ipv4: String,
    pub ipv6: String,
    pub connected: bool,
}

pub struct NetStat {
    pub speed: u64,       // Current bytes/sec
    pub top: u64,         // Peak bytes/sec
    pub total: u64,       // Total bytes transferred
    pub last: u64,        // Last raw counter value
    pub offset: u64,      // Reset offset
    pub rollover: u64,    // Counter rollover accumulator
}
```

#### 1.4 Process Domain (`domain/process.rs`)

```rust
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub cmd: String,
    pub short_cmd: String,
    pub threads: usize,
    pub user: String,
    pub mem: u64,                      // Working set bytes
    pub cpu_p: f64,                    // Current CPU %
    pub cpu_c: f64,                    // Cumulative CPU %
    pub state: ProcState,
    pub priority: PriorityClass,
    pub ppid: u32,
    pub start_time: u64,               // FILETIME as u64
    pub cpu_time: u64,                 // Total CPU time (100ns units)
    pub io_read: u64,
    pub io_write: u64,
    // Tree view fields
    pub prefix: String,
    pub depth: usize,
    pub tree_index: usize,
    pub collapsed: bool,
    pub filtered: bool,
}

pub enum ProcState {
    Running,
    Suspended,
    NotResponding,
    Unknown,
}

pub enum PriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

pub struct DetailContainer {
    pub last_pid: u32,
    pub entry: ProcInfo,
    pub elapsed: String,
    pub parent: String,
    pub status: String,
    pub io_read: String,
    pub io_write: String,
    pub memory: String,
    pub cpu_percent: VecDeque<i64>,
    pub mem_bytes: VecDeque<i64>,
}
```

#### 1.5 GPU Domain (`domain/gpu.rs`)

```rust
#[cfg(feature = "gpu")]
pub struct GpuInfo {
    pub name: String,
    pub gpu_percent: HashMap<String, VecDeque<i64>>,  // "gpu-totals","gpu-vram-totals","gpu-pwr-totals"
    pub gpu_clock_speed: u32,    // MHz
    pub mem_clock_speed: u64,    // MHz
    pub pwr_usage: i64,          // mW
    pub pwr_max_usage: i64,      // mW
    pub pwr_state: i64,
    pub temp: VecDeque<i64>,     // °C
    pub temp_max: i64,
    pub mem_total: u64,          // bytes
    pub mem_used: u64,           // bytes
    pub mem_utilization_percent: VecDeque<i64>,
    pub pcie_tx: u64,            // KB/s
    pub pcie_rx: u64,            // KB/s
    pub encoder_utilization: u64,   // %
    pub decoder_utilization: u64,   // %
    pub supported: GpuSupported,
}
```

#### Tests (Phase 1)

```
test domain::cpu::default_cpu_info_has_correct_keys
test domain::cpu::battery_info_default_is_no_battery
test domain::memory::mem_info_contains_all_stat_keys
test domain::memory::disk_info_percentages_valid_range
test domain::network::net_stat_default_zeroed
test domain::process::proc_state_display_names
test domain::process::priority_class_ordering
test domain::process::detail_container_default
test domain::gpu::gpu_info_default_supported_flags
```

---

### Phase 2: Tools, Logging & String Utilities

**Goal:** Implement all pure utility functions that the rest of the codebase depends on.

#### 2.1 String Utilities (`tools.rs`)

Replicate btop's string toolkit:

| Function | Purpose | btop equivalent |
|---|---|---|
| `ulen(s, wide) -> usize` | UTF-8 display width (respecting wide chars) | `ulen()` |
| `uresize(s, len, wide) -> String` | Truncate string to display width | `uresize()` |
| `luresize(s, len, wide) -> String` | Left-truncate to width | `luresize()` |
| `ljust(s, width) -> String` | Left-justify, pad with spaces | `ljust()` |
| `rjust(s, width) -> String` | Right-justify, pad with spaces | `rjust()` |
| `cjust(s, width) -> String` | Center-justify | `cjust()` |
| `floating_humanizer(value, shorten, bit, per_second) -> String` | Format bytes/bits (B, KiB, MiB, GiB, TiB) | `floating_humanizer()` |
| `sec_to_dhms(seconds, no_days, no_seconds) -> String` | Convert seconds to "XdHH:MM:SS" | `sec_to_dhms()` |
| `celsius_to(celsius, scale) -> (i64, String)` | Convert temp to F/K/R with unit suffix | `celsius_to()` |
| `strf_time(format) -> String` | strftime-compatible format with `/host`, `/user`, `/uptime` replacements | `strf_time()` |

#### 2.2 Logging (`log.rs`)

- Initialize `tracing` subscriber with file appender
- Log file: `%LOCALAPPDATA%\rtop\rtop.log`
- Rotation when >1MB (old → `.log.1`)
- Levels: ERROR, WARNING, INFO, DEBUG (matching btop)
- Log header with timestamp and version on startup

#### Tests (Phase 2)

```
# String utilities
test tools::ulen_ascii_string
test tools::ulen_cjk_characters_count_double
test tools::ulen_emoji_width
test tools::ulen_ansi_escape_codes_ignored
test tools::uresize_truncates_at_width
test tools::uresize_preserves_ansi_codes
test tools::luresize_removes_from_left
test tools::ljust_pads_right
test tools::rjust_pads_left
test tools::cjust_centers
test tools::ljust_truncates_if_over
test tools::floating_humanizer_bytes
test tools::floating_humanizer_kib
test tools::floating_humanizer_mib
test tools::floating_humanizer_gib
test tools::floating_humanizer_tib
test tools::floating_humanizer_shortened
test tools::floating_humanizer_bits
test tools::floating_humanizer_per_second
test tools::floating_humanizer_base10
test tools::sec_to_dhms_seconds_only
test tools::sec_to_dhms_minutes_seconds
test tools::sec_to_dhms_hours_minutes_seconds
test tools::sec_to_dhms_days
test tools::sec_to_dhms_no_days_flag
test tools::sec_to_dhms_no_seconds_flag
test tools::celsius_to_fahrenheit
test tools::celsius_to_kelvin
test tools::celsius_to_rankine
test tools::celsius_to_celsius_identity
test tools::strf_time_basic_format
test tools::strf_time_host_replacement
test tools::strf_time_user_replacement
test tools::strf_time_uptime_replacement

# Logging
test log::init_creates_log_file
test log::log_levels_filter_correctly
test log::log_rotation_on_size_exceeded
```

---

### Phase 3: Configuration System

**Goal:** Full configuration parsing/writing with all ~100 config keys, defaults, validation, and preset management. The config file format matches btop's `btop.conf` exactly.

#### 3.1 Config File Format

btop's config format:
```
#? Config file for btop v1.4.6

#* Name of a btop++/bpytop/bashtop formatted ".theme" file...
#* Themes should be placed in: ...
color_theme = "Default"

#* If the theme set background should be shown...
theme_background = True

#* ...
update_ms = 2000
```

Rules:
- Lines starting with `#` are comments (preserved on write)
- Key-value pairs: `key = value` or `key = "value"`
- Booleans: `True`/`False` (capitalized, matching btop)
- Integers: bare numbers
- Strings: quoted with `"`

#### 3.2 All Configuration Keys

**String configs (22 keys):**

| Key | Default | Validation |
|---|---|---|
| `color_theme` | `"Default"` | Theme name or path |
| `shown_boxes` | `"cpu mem net proc"` | Space-separated valid box names |
| `graph_symbol` | `"braille"` | `braille\|block\|tty` |
| `graph_symbol_cpu` | `"default"` | `default\|braille\|block\|tty` |
| `graph_symbol_gpu` | `"default"` | `default\|braille\|block\|tty` |
| `graph_symbol_mem` | `"default"` | `default\|braille\|block\|tty` |
| `graph_symbol_net` | `"default"` | `default\|braille\|block\|tty` |
| `graph_symbol_proc` | `"default"` | `default\|braille\|block\|tty` |
| `proc_sorting` | `"cpu lazy"` | `pid\|name\|command\|threads\|user\|memory\|cpu lazy\|cpu direct` |
| `cpu_graph_upper` | `"Auto"` | `Auto\|total\|user\|system\|idle\|irq\|dpc` (+ gpu fields if gpu feature) |
| `cpu_graph_lower` | `"Auto"` | Same as upper |
| `cpu_sensor` | `"Auto"` | `Auto` or sensor name |
| `selected_battery` | `"Auto"` | `Auto` or battery name |
| `cpu_core_map` | `""` | Custom mapping string |
| `temp_scale` | `"celsius"` | `celsius\|fahrenheit\|kelvin\|rankine` |
| `clock_format` | `"%X"` | strftime format string |
| `custom_cpu_name` | `""` | Freeform |
| `disks_filter` | `""` | Space-separated mount/drive filters |
| `io_graph_speeds` | `""` | Per-device speed limits |
| `net_iface` | `""` | Interface name or empty for auto |
| `log_level` | `"WARNING"` | `DISABLED\|ERROR\|WARNING\|INFO\|DEBUG` |
| `proc_filter` | `""` | Regex filter string |
| `presets` | `"..."` | Semicolon-separated preset definitions |
| `custom_gpu_name0` through `custom_gpu_name5` | `""` | GPU name overrides |

**Boolean configs (38 keys):**

| Key | Default |
|---|---|
| `theme_background` | `true` |
| `truecolor` | `true` |
| `rounded_corners` | `true` |
| `proc_reversed` | `false` |
| `proc_tree` | `false` |
| `proc_colors` | `true` |
| `proc_gradient` | `true` |
| `proc_per_core` | `false` |
| `proc_mem_bytes` | `true` |
| `proc_cpu_graphs` | `true` |
| `proc_left` | `false` |
| `proc_filter_kernel` | `false` |
| `proc_follow_detailed` | `true` |
| `proc_aggregate` | `false` |
| `keep_dead_proc_usage` | `false` |
| `cpu_invert_lower` | `true` |
| `cpu_single_graph` | `false` |
| `cpu_bottom` | `false` |
| `show_uptime` | `true` |
| `show_cpu_watts` | `true` |
| `check_temp` | `true` |
| `show_coretemp` | `true` |
| `show_cpu_freq` | `true` |
| `mem_graphs` | `true` |
| `mem_below_net` | `false` |
| `show_swap` | `true` |
| `swap_disk` | `true` |
| `show_disks` | `true` |
| `only_physical` | `true` |
| `show_io_stat` | `true` |
| `io_mode` | `false` |
| `io_graph_combined` | `false` |
| `swap_upload_download` | `false` |
| `base_10_sizes` | `false` |
| `net_auto` | `true` |
| `net_sync` | `true` |
| `show_battery` | `true` |
| `show_battery_watts` | `true` |
| `vim_keys` | `false` |
| `force_tty` | `false` |
| `lowcolor` | `false` |
| `background_update` | `true` |
| `terminal_sync` | `true` |
| `save_config_on_exit` | `true` |
| `disable_mouse` | `false` |
| `disk_free_priv` | `false` |
| `gpu_mirror_graph` | `true` |

**Integer configs (8 keys):**

| Key | Default | Range |
|---|---|---|
| `update_ms` | `2000` | 100 – 86,400,000 |
| `net_download` | `100` | 0 – 10,000,000 |
| `net_upload` | `100` | 0 – 10,000,000 |
| `detailed_pid` | `0` | ≥0 |
| `selected_pid` | `0` | ≥0 |
| `followed_pid` | `0` | ≥0 |
| `proc_start` | `0` | ≥0 |
| `proc_selected` | `0` | ≥0 |

**Linux-only configs (hidden/ignored on Windows):**
- `use_fstab`, `zfs_arc_cached`, `zfs_hide_datasets`, `proc_info_smaps`
- `freq_mode` (supported but simplified)
- `nvml_measure_pcie_speeds`, `rsmi_measure_pcie_speeds` (NVML available, RSMI mapped to ADLX)

#### 3.3 Preset System

- 10 presets (indices 0–9) stored as semicolon-separated strings
- Each preset defines `shown_boxes` layout
- Keys `p`/`P` cycle through presets
- Preset format: `"cpu:0:default,mem:0:default,net:0:default,proc:0:default"`

#### 3.4 Config Functions

```rust
pub fn load(path: &Path) -> Result<Config, Vec<String>>  // Returns warnings for invalid values
pub fn write(config: &Config, path: &Path) -> Result<()>
pub fn get_bool(name: &str) -> bool
pub fn get_int(name: &str) -> i32
pub fn get_string(name: &str) -> &str
pub fn set<T: ConfigValue>(name: &str, value: T)
pub fn flip(name: &str)                                   // Toggle boolean
pub fn toggle_box(box_name: &str) -> bool                 // Toggle box visibility
pub fn apply_preset(preset_str: &str) -> bool
pub fn valid_box_sizes(boxes: &str) -> bool
pub fn current_config() -> String                         // Generate default config text
```

#### Tests (Phase 3)

```
# Config parsing
test config::load_empty_file_uses_defaults
test config::load_valid_config_parses_all_keys
test config::load_preserves_comments
test config::load_invalid_key_generates_warning
test config::load_invalid_value_uses_default
test config::load_missing_file_creates_default
test config::write_produces_parseable_output
test config::write_roundtrip_preserves_values
test config::write_includes_comments

# Config accessors
test config::get_bool_returns_default
test config::get_bool_returns_set_value
test config::get_int_returns_default
test config::get_string_returns_default
test config::set_bool_updates_value
test config::set_int_clamps_to_range
test config::set_string_validates_options
test config::flip_toggles_boolean

# Box management
test config::toggle_box_adds_when_missing
test config::toggle_box_removes_when_present
test config::valid_box_sizes_minimum_dimensions
test config::shown_boxes_parsing

# Presets
test config::apply_preset_valid
test config::apply_preset_invalid_returns_false
test config::preset_cycle_wraps_around
test config::preset_format_parsing
```

---

### Phase 4: Theme System

**Goal:** Complete theme file parsing, color conversion, and gradient generation matching btop exactly.

#### 4.1 Theme File Format

btop `.theme` files:
```
# Theme: Dracula
theme[main_bg]="#282a36"
theme[main_fg]="#f8f8f2"
theme[cpu_start]="#50fa7b"
theme[cpu_mid]=""
theme[cpu_end]="#ff5555"
```

#### 4.2 All Color Keys (35 base keys, ~54 with _start/_mid/_end variants)

**Flat colors (15):**
`main_bg`, `main_fg`, `title`, `hi_fg`, `selected_bg`, `selected_fg`, `inactive_fg`, `graph_text`, `meter_bg`, `cpu_box`, `mem_box`, `net_box`, `proc_box`, `div_line`, `proc_misc`

**Process box special colors (6):**
`proc_pause_bg`, `proc_follow_bg`, `proc_banner_bg`, `proc_banner_fg`, `followed_bg`, `followed_fg`

**Gradient colors (9 gradients × 3 = 27):**
`temp_start/mid/end`, `cpu_start/mid/end`, `free_start/mid/end`, `cached_start/mid/end`, `available_start/mid/end`, `used_start/mid/end`, `download_start/mid/end`, `upload_start/mid/end`, `process_start/mid/end`

#### 4.3 Color Conversion

```rust
pub fn hex_to_color(hex: &str, to_256: bool, depth: ColorDepth) -> String  // #RRGGBB or #GG → ANSI escape
pub fn dec_to_color(r: u8, g: u8, b: u8, to_256: bool, depth: ColorDepth) -> String
pub fn truecolor_to_256(r: u8, g: u8, b: u8) -> u8  // 24-bit → 256-color conversion
```

#### 4.4 Gradient Generation Algorithm

1. Parse `_start`, `_mid`, `_end` for each gradient name
2. Generate 101-element array (indices 0–100) of ANSI color codes
3. Interpolation:
   - Start only → fill all 101 with start color
   - Start + end → linear interpolate across 0–100
   - Start + mid + end → 0–50 interpolate start→mid, 50–100 interpolate mid→end

#### 4.5 Default Theme & TTY Theme

- Default theme: full RGB hex values for all keys (hardcoded)
- TTY theme: 16-color ANSI escape codes (for legacy terminals)
- Fallback chain: `meter_bg`→`inactive_fg`, `process_*`→`cpu_*`, `graph_text`→`inactive_fg`

#### 4.6 Theme Discovery

Search paths (Windows):
1. CLI `--themes-dir` path
2. `%APPDATA%\rtop\themes\`
3. Bundled themes (embedded in binary via `include_str!` or resource)

#### Tests (Phase 4)

```
# Color conversion
test theme::hex_to_color_6char_truecolor
test theme::hex_to_color_6char_256color
test theme::hex_to_color_2char_grayscale
test theme::hex_to_color_invalid_returns_empty
test theme::dec_to_color_clamps_values
test theme::truecolor_to_256_grayscale
test theme::truecolor_to_256_color_cube
test theme::truecolor_to_256_near_black
test theme::truecolor_to_256_near_white

# Gradient generation
test theme::gradient_start_only_fills_all_101
test theme::gradient_start_end_linear_interpolation
test theme::gradient_start_mid_end_two_segment
test theme::gradient_values_at_boundaries
test theme::gradient_midpoint_equals_mid_color
test theme::gradient_101_elements_always

# Theme file parsing
test theme::parse_dracula_theme
test theme::parse_nord_theme
test theme::parse_comments_ignored
test theme::parse_unknown_keys_ignored
test theme::parse_empty_values_use_default
test theme::parse_rgb_decimal_format
test theme::parse_hex_format
test theme::parse_missing_keys_use_fallback

# Theme management
test theme::set_theme_default
test theme::set_theme_tty
test theme::set_theme_from_file
test theme::update_themes_discovers_files
test theme::fallback_colors_applied
test theme::tty_theme_uses_16_colors

# Bundled themes
test theme::all_40_bundled_themes_parse_without_error
test theme::bundled_theme_names_match_filenames
```

---

### Phase 5: Off-Screen Cell Buffer

**Goal:** Create a terminal-independent rendering buffer that all UI components write to. This enables deterministic snapshot testing without a real terminal.

#### 5.1 Cell & Buffer

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,        // Foreground color (RGB or named)
    pub bg: Color,        // Background color
    pub attrs: CellAttrs, // Bold, italic, underline, etc.
}

pub struct CellBuffer {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}
```

#### 5.2 Buffer Operations

```rust
impl CellBuffer {
    pub fn new(width: usize, height: usize) -> Self
    pub fn resize(&mut self, width: usize, height: usize)
    pub fn get(&self, x: usize, y: usize) -> &Cell
    pub fn set(&mut self, x: usize, y: usize, cell: Cell)
    pub fn put_str(&mut self, x: usize, y: usize, s: &str, fg: Color, bg: Color)
    pub fn put_ansi(&mut self, x: usize, y: usize, ansi_str: &str)  // Parse ANSI codes
    pub fn fill_row(&mut self, y: usize, ch: char, fg: Color, bg: Color)
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, ch: char, fg: Color, bg: Color)
    pub fn clear(&mut self)
    pub fn diff(&self, other: &CellBuffer) -> Vec<CellDiff>  // For minimal repaints
    pub fn to_ansi_string(&self) -> String  // Serialize to ANSI escape sequence stream
    pub fn snapshot(&self) -> String        // Plain text snapshot (for test assertions)
}
```

#### Tests (Phase 5)

```
test cell_buffer::new_creates_correct_size
test cell_buffer::resize_preserves_content
test cell_buffer::set_get_roundtrip
test cell_buffer::put_str_writes_characters
test cell_buffer::put_str_respects_bounds
test cell_buffer::put_str_handles_wide_chars
test cell_buffer::fill_row_fills_entire_row
test cell_buffer::fill_rect_fills_area
test cell_buffer::clear_resets_all_cells
test cell_buffer::diff_detects_changes
test cell_buffer::diff_empty_when_identical
test cell_buffer::to_ansi_string_produces_valid_output
test cell_buffer::snapshot_matches_expected_layout
test cell_buffer::put_ansi_parses_colors
test cell_buffer::put_ansi_handles_nested_codes
```

---

### Phase 6: Terminal Backend

**Goal:** Abstract terminal I/O using crossterm, providing initialization, raw mode, alternate screen, color output, resize detection, and cursor control. All output goes through the cell buffer → terminal flush pipeline.

#### 6.1 Terminal Abstraction (`term.rs`)

```rust
pub struct Terminal {
    width: u16,
    height: u16,
    truecolor: bool,
    tty_mode: bool,
}

impl Terminal {
    pub fn init() -> Result<Self>           // Raw mode, alternate screen, hide cursor, mouse on
    pub fn restore(&self)                    // Normal screen, show cursor, mouse off
    pub fn refresh(&self) -> bool            // Check if terminal size changed
    pub fn size(&self) -> (u16, u16)         // (width, height)
    pub fn flush_buffer(&self, buf: &CellBuffer, prev: &CellBuffer)  // Diff-based flush
    pub fn flush_full(&self, buf: &CellBuffer)  // Full redraw
    pub fn set_title(&self, title: &str)
}
```

#### 6.2 ANSI Escape Code Constants

```rust
pub mod fx {
    pub const BOLD: &str = "\x1b[1m";
    pub const UNBOLD: &str = "\x1b[22m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const BLINK: &str = "\x1b[5m";
    pub const STRIKETHROUGH: &str = "\x1b[9m";
    pub const RESET: &str = "\x1b[0m";
    // ... all matching btop's Fx namespace
}

pub mod mv {
    pub fn to(line: u16, col: u16) -> String  // "\x1b[{line};{col}H"
    pub fn right(n: u16) -> String
    pub fn left(n: u16) -> String
    pub fn up(n: u16) -> String
    pub fn down(n: u16) -> String
    pub const SAVE: &str = "\x1b[s";
    pub const RESTORE: &str = "\x1b[u";
}
```

#### 6.3 Synchronized Output

```rust
pub fn sync_start() -> &'static str  // "\x1b[?2026h"
pub fn sync_end() -> &'static str    // "\x1b[?2026l"
```

#### Tests (Phase 6)

```
test term::init_enters_raw_mode
test term::restore_exits_raw_mode
test term::size_returns_positive_dimensions
test term::escape_code_constants_valid
test term::mv_to_produces_correct_sequence
test term::mv_right_left_up_down
test term::flush_buffer_uses_diff
test term::flush_full_writes_entire_buffer
```

---

### Phase 7: Input System

**Goal:** Translate Windows console input events (via crossterm) into the key names btop uses, handle mouse events, and route actions to the correct handler.

#### 7.1 Key Translation Map

Map crossterm `KeyEvent` / `MouseEvent` to btop key names:

| Input | btop Name |
|---|---|
| `Esc` | `"escape"` |
| `Enter` | `"enter"` |
| `Space` | `"space"` |
| `Backspace` | `"backspace"` |
| `Up` | `"up"` |
| `Down` | `"down"` |
| `Left` | `"left"` |
| `Right` | `"right"` |
| `Insert` | `"insert"` |
| `Delete` | `"delete"` |
| `Home` | `"home"` |
| `End` | `"end"` |
| `PageUp` | `"page_up"` |
| `PageDown` | `"page_down"` |
| `Tab` | `"tab"` |
| `BackTab` | `"shift_tab"` |
| `F(1)`–`F(12)` | `"f1"`–`"f12"` |
| `Ctrl+'r'` | `"ctrl_r"` |
| Mouse left click | `"mouse_click"` |
| Mouse drag | `"mouse_drag"` |
| Mouse release | `"mouse_release"` |
| Scroll up | `"mouse_scroll_up"` |
| Scroll down | `"mouse_scroll_down"` |

#### 7.2 Mouse Mapping

```rust
pub struct MouseLoc {
    pub line: u16,
    pub col: u16,
    pub height: u16,
    pub width: u16,
}

/// Map of clickable region name → location
pub type MouseMappings = HashMap<String, MouseLoc>;
```

#### 7.3 Input Processing

```rust
pub fn poll(timeout_ms: u64) -> bool
pub fn get() -> Option<String>            // Returns translated key name
pub fn process(key: &str, state: &mut AppState)  // Route action based on current context
```

#### 7.4 Complete Keybind Table

**Global keys (always active unless filtering):**

| Key | Action |
|---|---|
| `q` | Quit application |
| `escape` / `m` | Toggle main menu |
| `f1` / `?` / `h` (or `H` with vim_keys) | Show help menu |
| `f2` / `o` | Show options menu |
| `1` | Toggle CPU box |
| `2` | Toggle MEM box |
| `3` | Toggle NET box |
| `4` | Toggle PROC box |
| `5`–`0` | Toggle GPU boxes 0–5 (if GPU feature enabled) |
| `p` | Next preset |
| `P` | Previous preset |
| `ctrl_r` | Reload config file |
| `+` | Increase update rate (+100ms, +1000ms if held) |
| `-` | Decrease update rate (-100ms, -1000ms if held) |

**CPU box keys:**
(Update rate +/- handled in global)

**MEM box keys:**

| Key | Action |
|---|---|
| `i` | Toggle IO mode |
| `d` | Toggle disk view |

**NET box keys:**

| Key | Action |
|---|---|
| `b` | Previous network interface |
| `n` | Next network interface |
| `y` | Toggle sync scaling |
| `a` | Toggle auto-scaling |
| `z` | Reset totals for current interface |

**PROC box keys:**

| Key | Action |
|---|---|
| `left` / `h` (vim) | Previous sort column |
| `right` / `l` (vim) | Next sort column |
| `f` / `/` | Toggle filter mode |
| `e` | Toggle tree view |
| `u` | Pause/resume list |
| `F` | Follow selected process |
| `r` | Reverse sort order |
| `c` | Toggle per-core CPU |
| `%` | Toggle memory bytes/percent |
| `delete` | Clear filter |
| `up` / `k` (vim) | Select previous process |
| `down` / `j` (vim) | Select next process |
| `page_up` | Page up |
| `page_down` | Page down |
| `home` / `g` (vim) | Go to first process |
| `end` / `G` (vim) | Go to last process |
| `enter` | Show/hide detailed view |
| `space` / `+` / `C` (tree) | Expand tree node |
| `-` (tree) | Collapse tree node |
| `t` | Terminate process (`TerminateProcess`) |
| `k` / `K` (vim) | Kill process (`TerminateProcess` with force) |
| `s` | Show process action menu (terminate/suspend/resume/end task) |
| `N` | Show priority change menu |

**Filter mode keys:**

| Key | Action |
|---|---|
| any char | Append to filter |
| `backspace` | Delete char before cursor |
| `delete` | Delete char at cursor |
| `left` / `right` / `home` / `end` | Move cursor |
| `enter` / `down` | Accept filter |
| `escape` | Cancel filter |

#### Tests (Phase 7)

```
test input::translate_escape_key
test input::translate_arrow_keys
test input::translate_function_keys
test input::translate_ctrl_r
test input::translate_mouse_click
test input::translate_mouse_scroll
test input::translate_regular_char
test input::translate_backspace
test input::process_q_triggers_quit
test input::process_m_toggles_menu
test input::process_1_toggles_cpu_box
test input::process_vim_keys_when_enabled
test input::process_vim_keys_disabled_by_default
test input::mouse_loc_contains_point
test input::mouse_mapping_lookup
test input::filter_mode_accepts_chars
test input::filter_mode_escape_cancels
test input::filter_mode_enter_applies
```

---

### Phase 8: Drawing Primitives

**Goal:** Implement Graph, Meter, TextEdit, createBox, and calcSizes — the core rendering building blocks.

#### 8.1 Graph (`draw/graph.rs`)

Braille graph encoding: each character cell is a 2×4 dot matrix (Unicode braille patterns U+2800–U+28FF). btop uses a 5×5 lookup table indexed by (previous_value, current_value) to select the correct braille character.

```rust
pub struct Graph {
    width: usize,
    height: usize,
    color_gradient: String,
    symbol: GraphSymbol,
    invert: bool,
    no_zero: bool,
    max_value: i64,
    offset: i64,
    // Internal state
    graphs: [Vec<String>; 2],  // Double-buffered graph lines
    current: bool,              // Which buffer is active
    last: i64,
    tty_mode: bool,
}

pub enum GraphSymbol {
    Braille,
    Block,
    Tty,
}
```

**Symbol tables (25 entries each, matching btop):**

```rust
const BRAILLE_UP: [&str; 25] = [" ", "⢀", "⢠", "⢰", "⢸", "⡀", "⣀", "⣠", "⣰", "⣸", "⡄", "⣄", "⣤", "⣴", "⣼", "⡆", "⣆", "⣦", "⣶", "⣾", "⡇", "⣇", "⣧", "⣷", "⣿"];
const BRAILLE_DOWN: [&str; 25] = [" ", "⠈", "⠘", "⠸", "⢸", "⠁", "⠉", "⠙", "⠹", "⢹", "⠃", "⠋", "⠛", "⠻", "⢻", "⠇", "⠏", "⠟", "⠿", "⢿", "⡇", "⡏", "⡟", "⡿", "⣿"];
const BLOCK_UP: [&str; 25] = [" ", "▗", "▗", "▐", "▐", "▖", "▄", "▄", "▟", "▟", "▖", "▄", "▄", "▟", "▟", "▌", "▙", "▙", "█", "█", "▌", "▙", "▙", "█", "█"];
const BLOCK_DOWN: [&str; 25] = [" ", "▝", "▝", "▐", "▐", "▘", "▀", "▀", "▜", "▜", "▘", "▀", "▀", "▜", "▜", "▌", "▛", "▛", "█", "█", "▌", "▛", "▛", "█", "█"];
const TTY_UP: [&str; 25] = [" ", "░", "░", "▒", "▒", "░", "░", "▒", "▒", "█", "░", "░", "▒", "▒", "█", "▒", "▒", "▒", "█", "█", "█", "█", "█", "█", "█"];
const TTY_DOWN: [&str; 25] = [" ", "░", "░", "▒", "▒", "░", "░", "▒", "▒", "█", "░", "░", "▒", "▒", "█", "▒", "▒", "▒", "█", "█", "█", "█", "█", "█", "█"];
```

**Graph rendering algorithm:**
1. Normalize data values to 0–(height×2-1) range (braille has 2 rows per character)
2. For each column, look up symbol[prev_val * 5 + curr_val] (5 levels per half)
3. Apply color gradient based on value level (0–100)
4. Maintain double buffer for smooth animation (swap on each update)
5. Shift existing graph left by 1, append new column

#### 8.2 Meter (`draw/meter.rs`)

```rust
pub struct Meter {
    width: usize,
    color_gradient: String,
    invert: bool,
    cache: [String; 101],  // Pre-computed for values 0–100
}

impl Meter {
    pub fn new(width: usize, color_gradient: &str, invert: bool) -> Self
    pub fn render(&self, value: i32) -> &str  // Returns cached ANSI string
}
```

Rendering: fill `width * value / 100` cells with `■` colored from gradient, rest with `meter_bg` color.

#### 8.3 TextEdit (`draw/text_edit.rs`)

```rust
pub struct TextEdit {
    pub text: String,
    pos: usize,       // Byte position
    upos: usize,      // Character position
    numeric: bool,
}

impl TextEdit {
    pub fn new(text: String, numeric: bool) -> Self
    pub fn command(&mut self, key: &str) -> bool  // Process key, return true if changed
    pub fn render(&self, limit: usize) -> String   // Render with cursor underline
    pub fn clear(&mut self)
}
```

#### 8.4 Box Drawing (`draw/box_drawing.rs`)

```rust
pub fn create_box(
    x: usize, y: usize,
    width: usize, height: usize,
    line_color: &str,
    fill: bool,
    title: &str,
    title2: &str,
    num: u8,
) -> String
```

**Box characters:**

| Normal | Rounded | TTY |
|---|---|---|
| `┌ ┐ └ ┘` | `╭ ╮ ╰ ╯` | `┌ ┐ └ ┘` |
| `─ │` | `─ │` | `- \|` |
| `├ ┤ ┬ ┴` | `├ ┤ ┬ ┴` | `├ ┤ ┬ ┴` |
| `╎` (dotted) | `╎` | `:` |

Superscript numbers: `⁰ ¹ ² ³ ⁴ ⁵ ⁶ ⁷ ⁸ ⁹`

#### 8.5 Layout Calculator (`draw/layout.rs`)

```rust
pub struct BoxDimensions {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

pub struct Layout {
    pub cpu: Option<BoxDimensions>,
    pub mem: Option<BoxDimensions>,
    pub net: Option<BoxDimensions>,
    pub proc_box: Option<BoxDimensions>,
    pub gpu: Vec<BoxDimensions>,  // 0–6 GPU boxes
}

pub fn calc_sizes(
    term_width: usize,
    term_height: usize,
    shown_boxes: &[String],
    config: &Config,
    core_count: usize,
    gpu_count: usize,
) -> Layout
```

**Layout rules (matching btop):**
- CPU: top (or bottom if `cpu_bottom`), height based on core count and column size
- GPU: stacked below CPU, each gets equal share of GPU space
- MEM: left side, 40–45% width, height based on `mem_below_net` setting
- NET: paired with MEM (above or below based on config)
- PROC: right side, 55% width, full height below CPU
- Minimum sizes: CPU 8h, MEM 10h×36w, NET 6h×20w, PROC 10h×44w, GPU 8h×41w

#### Tests (Phase 8)

```
# Graph
test graph::new_creates_correct_dimensions
test graph::braille_symbol_lookup_correct
test graph::block_symbol_lookup_correct
test graph::tty_symbol_lookup_correct
test graph::render_single_value_100_percent
test graph::render_single_value_0_percent
test graph::render_single_value_50_percent
test graph::render_applies_color_gradient
test graph::render_inverted_flips_direction
test graph::render_no_zero_skips_zero_values
test graph::render_max_value_clamping
test graph::render_offset_subtracted
test graph::render_width_matches_data_length
test graph::double_buffer_alternates
test graph::shift_left_on_new_data
test graph::snapshot_braille_ascending_data
test graph::snapshot_braille_descending_data
test graph::snapshot_block_data

# Meter
test meter::render_0_percent_empty
test meter::render_100_percent_full
test meter::render_50_percent_half
test meter::render_cache_hit
test meter::render_gradient_colors_applied
test meter::render_inverted_direction

# TextEdit
test text_edit::command_left_moves_cursor
test text_edit::command_right_moves_cursor
test text_edit::command_home_goes_to_start
test text_edit::command_end_goes_to_end
test text_edit::command_backspace_deletes
test text_edit::command_delete_removes_at_cursor
test text_edit::command_char_inserts
test text_edit::command_numeric_rejects_non_digits
test text_edit::render_shows_cursor_underline
test text_edit::render_truncates_to_limit

# Box drawing
test box_drawing::create_box_minimal
test box_drawing::create_box_with_title
test box_drawing::create_box_with_number
test box_drawing::create_box_rounded_corners
test box_drawing::create_box_fill
test box_drawing::create_box_tty_mode

# Layout
test layout::calc_sizes_all_boxes_shown
test layout::calc_sizes_cpu_only
test layout::calc_sizes_proc_only
test layout::calc_sizes_cpu_bottom
test layout::calc_sizes_proc_left
test layout::calc_sizes_mem_below_net
test layout::calc_sizes_minimum_terminal_size
test layout::calc_sizes_with_gpu_boxes
test layout::calc_sizes_core_count_affects_cpu_height
test layout::calc_sizes_respects_minimum_dimensions
```

---

### Phase 9: Windows System Collectors

**Goal:** Implement all data collection modules using Windows APIs. Each collector fills the corresponding domain model struct.

#### 9.1 Collector Trait

```rust
pub trait Collector {
    type Output;
    fn collect(&mut self, no_update: bool) -> &Self::Output;
}
```

#### 9.2 CPU Collector (`collect/cpu.rs`)

**APIs used:**
- `NtQuerySystemInformation(SystemProcessorPerformanceInformation)` — per-core Idle/Kernel/User/Dpc/Interrupt times
- Registry `HKLM\HARDWARE\...\ProcessorNameString` — CPU model name
- `CallNtPowerInformation(ProcessorInformation)` — per-core current/max frequency
- `GetSystemPowerStatus()` — battery status
- `GetTickCount64()` — system uptime
- WMI `MSAcpi_ThermalZoneTemperature` — CPU temperature (requires admin)
- LibreHardwareMonitor WMI bridge (optional) — detailed per-core temps

**CPU percentage calculation:**
```
idle_delta = new_idle - old_idle
kernel_delta = new_kernel - old_kernel  // KernelTime includes IdleTime
user_delta = new_user - old_user
total_delta = kernel_delta + user_delta
cpu_percent = ((total_delta - idle_delta) * 100) / total_delta
```

**Load average emulation:**
- Maintain rolling window of CPU % samples
- 1-min avg = average of last 60s of samples
- 5-min avg = average of last 300s of samples
- 15-min avg = average of last 900s of samples
- Use exponential moving average (EMA) for efficiency: `load = load * decay + sample * (1 - decay)`

#### 9.3 Memory Collector (`collect/memory.rs`)

**APIs used:**
- `GlobalMemoryStatusEx()` — total/available physical RAM, total/available page file
- `GetPerformanceInfo()` — cache pages, page size
- `GetLogicalDriveStringsW()` — enumerate drives
- `GetVolumeInformationW()` — drive label, fstype
- `GetDiskFreeSpaceExW()` — total/free per drive
- PDH `\PhysicalDisk(*)\Disk Read Bytes/sec` — disk IO

**Memory mapping:**
```
total = GlobalMemoryStatusEx.ullTotalPhys
available = GlobalMemoryStatusEx.ullAvailPhys
cached = GetPerformanceInfo.SystemCache * PageSize
free = available (Windows doesn't distinguish free vs available cleanly)
used = total - available
swap_total = PageFile.ullTotalPageFile - total
swap_used = swap_total - (PageFile.ullAvailPageFile - available)
swap_free = swap_total - swap_used
```

#### 9.4 Network Collector (`collect/network.rs`)

**APIs used:**
- `GetAdaptersAddresses(AF_UNSPEC)` — interface enumeration, IPs, status
- `GetIfEntry2()` — per-interface `InOctets`, `OutOctets` counters
- Delta calculation: `speed = (new_octets - old_octets) / delta_time_seconds`

**Interface filtering:**
- Skip loopback interfaces
- Skip interfaces with `IfType == IF_TYPE_SOFTWARE_LOOPBACK`
- Only show interfaces with `OperStatus == IfOperStatusUp` by default

#### 9.5 Process Collector (`collect/process.rs`)

**APIs used:**
- `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` + `Process32FirstW/NextW` — enumerate PIDs, names, PPIDs, threads
- `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` — handle for queries
- `GetProcessTimes()` — creation time, kernel time, user time
- `GetProcessMemoryInfo()` — working set size
- `GetProcessIoCounters()` — IO read/write bytes
- `OpenProcessToken()` + `GetTokenInformation(TokenUser)` + `LookupAccountSidW()` — username
- `NtQueryInformationProcess(ProcessBasicInformation)` → PEB → `CommandLine` — command line
- `GetPriorityClass()` — priority
- `IsProcessInJob()` — job membership
- `NtSuspendProcess()` / `NtResumeProcess()` — suspend/resume
- `TerminateProcess()` — kill

**CPU % calculation per process:**
```
process_time_delta = (new_kernel + new_user) - (old_kernel + old_user)  // in 100ns units
system_time_delta = time_elapsed * 10_000_000  // seconds to 100ns
cpu_percent = (process_time_delta * 100) / system_time_delta
// Optionally normalize by core count: cpu_percent /= core_count
```

**Process tree building:**
1. Collect all processes with PPID
2. Build adjacency list: `HashMap<u32, Vec<u32>>` (parent → children)
3. Sort children by the selected sort column
4. DFS traversal to generate flat list with depth/prefix information
5. Prefix characters: `├─`, `└─`, `│ `, `  ` (matching btop tree view)

**Process actions menu (replaces Linux signals):**

| Action | Implementation | btop equivalent |
|---|---|---|
| Terminate | `TerminateProcess(handle, 1)` | SIGKILL (9) |
| End Task | `PostMessage(hwnd, WM_CLOSE, 0, 0)` via `EnumWindows` | SIGTERM (15) |
| Suspend | `NtSuspendProcess(handle)` | SIGSTOP (19) |
| Resume | `NtResumeProcess(handle)` | SIGCONT (18) |

#### 9.6 GPU Collector (`collect/gpu.rs`) — Feature-gated

**NVIDIA (NVML):**
- Dynamically load `nvml.dll` from `C:\Windows\System32\nvml.dll` (ships with driver)
- Same API as Linux: `nvmlInit`, `nvmlDeviceGetCount`, `nvmlDeviceGetUtilizationRates`, etc.
- All metrics available: utilization, temp, VRAM, power, clocks, PCIe, encoder/decoder

**AMD (ADLX):**
- Load `amdadlx64.dll` dynamically
- Query `IADLXPerformanceMonitoringServices` for GPU metrics
- Subset of NVML metrics available

**Intel (D3DKMT):**
- Use `D3DKMTQueryStatistics` from `gdi32.dll`
- Basic utilization only

**Fallback:** Use WMI `Win32_VideoController` for basic GPU name/VRAM info when vendor SDKs unavailable.

#### Test Strategy (Phase 9)

Collectors are tested in three tiers:

**Tier 1: Unit tests (pure logic, no OS calls)**
```
test cpu::calculate_cpu_percent_delta
test cpu::calculate_per_core_percent
test cpu::load_average_ema_calculation
test cpu::frequency_format_ghz
test cpu::frequency_format_mhz
test memory::calculate_used_from_total_available
test memory::calculate_swap_from_pagefile
test memory::disk_percent_calculation
test network::speed_from_octet_delta
test network::rollover_handling
test network::total_with_offset
test process::cpu_percent_from_times
test process::build_tree_from_ppid_map
test process::tree_prefix_generation
test process::sort_by_cpu
test process::sort_by_memory
test process::sort_by_name
test process::sort_by_pid
test process::filter_regex_match
test process::filter_regex_negation
test process::priority_class_from_u32
```

**Tier 2: Fixture/snapshot tests (replay captured data)**
```
test cpu::parse_fixture_processor_info
test memory::parse_fixture_drive_info
test network::parse_fixture_adapter_info
test process::parse_fixture_process_list
```

**Tier 3: Integration tests (real system, opt-in)**
```
#[cfg(test)] #[ignore]  // Run with `cargo test -- --ignored`
test cpu::collect_returns_valid_cpu_info
test memory::collect_returns_valid_mem_info
test network::collect_returns_at_least_one_interface
test process::collect_returns_current_process
test process::collect_returns_explorer_exe
```

---

### Phase 10: UI Box Renderers

**Goal:** Implement each UI box as a renderer that reads domain data and writes to the cell buffer.

#### 10.1 CPU Box (`ui/cpu_box.rs`)

**Layout:**
```
╭──────────────────────────── cpu ─╮
│ CPU Intel Core i9-13900K 5.8GHz  │
│ ⣿⣷⣤⣠⣀⡀ ... ⣿⣷⣤⣠⣀⡀        82% │ ← Upper graph
│ ⣿⣷⣤⣠⣀⡀ ... ⣿⣷⣤⣠⣀⡀        45% │ ← Lower graph (inverted)
│                                  │
│ 0 ■■■■■■■░░░ 58%  8 ■■■■░░ 32%  │ ← Core meters/mini-graphs
│ 1 ■■■■■■■■░░ 72%  9 ■■■░░░ 24%  │
│ ...                              │
│ Tm 62°C  Up 3d12:45  ▪ 87% ⌁    │ ← Temp, uptime, battery
╰─ m menu ─── p preset ─── -/+ ──╯
```

**Components:**
- Upper graph: configurable stat (`cpu_graph_upper`)
- Lower graph: configurable stat (`cpu_graph_lower`), optionally inverted
- Core grid: `b_columns × rows` of mini meters or mini graphs
- Temperature display (per-core or package only)
- CPU frequency
- Load average
- Battery meter
- Clock display (formatted with `clock_format`)
- Clickable buttons: menu (m), preset (p), update rate (-/+)

#### 10.2 Memory Box (`ui/mem_box.rs`)

**Layout:**
```
╭─ mem ──────────────╮╭─ disks ─────────╮
│ Used  ■■■■■■░░ 62% ││ C: NTFS         │
│ Avail ■■░░░░░░ 38% ││ ■■■■■■■░ 250GiB │
│ Cache ■■■░░░░░ 25% ││   /439GiB  57%  │
│ Free  ■░░░░░░░ 12% ││                  │
│                    ││ D: NTFS          │
│ Swap  ■■░░░░░░ 18% ││ ■■■░░░░░ 1.2TiB │
│  3.2G / 16.0G      ││   /3.6TiB  33%  │
╰────────────────────╯╰──────────────────╯
```

**Modes:**
- Meters vs graphs (config `mem_graphs`)
- Show/hide disks (config `show_disks`)
- IO mode: activity bars → full IO read/write graphs
- Swap as separate section or as disk item

#### 10.3 Network Box (`ui/net_box.rs`)

**Layout:**
```
╭─ net ──────────────────────────╮
│ ⣿⣷⣤⣠ ... ⣿⣷⣤⣠⣀⡀    ▼ 12.5MiB/s │
│                                │
│ ⣿⣷⣤⣠ ... ⣿⣷⣤⣠⣀⡀    ▲  2.1MiB/s │
│ < Ethernet >  Total: 1.23 GiB │
╰─ b ◀ ── n ▶ ── y ⇅ ── a ▤ ──╯
```

**Features:**
- Download graph (upper) + upload graph (lower)
- Interface selector with `b`/`n` keys
- Auto-scale or fixed scale
- Sync mode (same scale for up/down)
- Stats box with speed, total, peak
- Interface IP address display

#### 10.4 Process Box (`ui/proc_box.rs`)

**Layout (normal mode):**
```
╭─ proc ─────────────────────────────────╮
│ PID    Program       Cpu%  Mem%  User  │
│  1234  explorer.exe  2.1   0.8   SYSTEM│
│  5678  chrome.exe    15.3  4.2   User  │
│  ...                                   │
│ « Filter: chrome »              1/243  │
╰─ ← sort → ── f filter ── e tree ─────╯
```

**Layout (tree mode):**
```
│ PID    Program              Cpu%  Mem% │
│     4  System                0.1  0.0 │
│  ├─ 128  smss.exe            0.0  0.0 │
│  └─ 512  csrss.exe           0.1  0.0 │
│  1234  explorer.exe          2.1  0.8 │
│  ├─ 2345  chrome.exe        15.3  4.2 │
│  │  ├─ 2346  chrome.exe      3.1  1.2 │
│  │  └─ 2347  chrome.exe      2.0  0.8 │
│  └─ 3456  code.exe           5.2  2.1 │
```

**Layout (detailed view):**
```
╭─ pid:1234 ── chrome.exe ───────────────╮
│ ⣿⣷⣤⣠⣀⡀ ... CPU 15.3%               │
│ ⣿⣷⣤⣠⣀⡀ ... Mem 4.2% (672MiB)       │
│ Status: Running   User: JohnDoe        │
│ Cmd: "C:\Program Files\Google\..."      │
│ Threads: 42   Parent: explorer.exe      │
│ IO R: 1.2MiB  IO W: 0.5MiB             │
│ Started: 3h12m ago  Priority: Normal    │
╰────────────────────────────────────────╯
```

**Features:**
- Column auto-sizing based on terminal width
- Process coloring by CPU usage (gradient)
- Selection highlight with background color
- Scrollbar
- Tree view with expand/collapse
- Filtering (regex with `!` prefix)
- Follow mode (track selected process)
- Pause mode (freeze updates)
- Mini CPU graphs per process (if wide enough and `proc_cpu_graphs` enabled)
- Sorting by: pid, name, command, threads, user, memory, cpu lazy, cpu direct

#### 10.5 GPU Box (`ui/gpu_box.rs`)

**Layout:**
```
╭─ gpu0 ── NVIDIA RTX 4090 ─────────────╮
│ ⣿⣷⣤⣠ ... ⣿⣷  GPU 78%  🌡 65°C       │
│ ⣿⣷⣤⣠ ... ⣿⣷  MEM 45%  ⚡ 320W       │
│ VRAM: 8.2G/24.0G  Clock: 2520MHz      │
│ PCIe TX: 1.2GB/s  RX: 0.8GB/s         │
│ Enc: 0%  Dec: 0%                       │
╰────────────────────────────────────────╯
```

#### Tests (Phase 10)

Each box renderer is tested with snapshot tests against the cell buffer:

```
# CPU Box
test cpu_box::render_basic_layout
test cpu_box::render_with_temperature
test cpu_box::render_with_battery
test cpu_box::render_core_grid_2_columns
test cpu_box::render_core_grid_4_columns
test cpu_box::render_single_graph_mode
test cpu_box::render_inverted_lower_graph
test cpu_box::render_clock_display
test cpu_box::render_minimum_size
test cpu_box::render_buttons

# Memory Box
test mem_box::render_meters_mode
test mem_box::render_graphs_mode
test mem_box::render_with_disks
test mem_box::render_without_disks
test mem_box::render_with_swap
test mem_box::render_io_mode
test mem_box::render_io_combined
test mem_box::render_minimum_size

# Network Box
test net_box::render_basic
test net_box::render_no_interface
test net_box::render_auto_scale
test net_box::render_sync_mode
test net_box::render_swap_upload_download
test net_box::render_interface_stats

# Process Box
test proc_box::render_basic_list
test proc_box::render_tree_view
test proc_box::render_with_selection
test proc_box::render_with_filter
test proc_box::render_detailed_view
test proc_box::render_follow_mode
test proc_box::render_paused
test proc_box::render_scrollbar
test proc_box::render_colors_gradient
test proc_box::render_sort_indicator
test proc_box::render_minimum_columns
test proc_box::render_wide_columns

# GPU Box
test gpu_box::render_nvidia_full
test gpu_box::render_limited_metrics
test gpu_box::render_minimum_size
```

---

### Phase 11: Menu System

**Goal:** Implement all menu overlays matching btop's menus exactly.

#### 11.1 Main Menu (`menu/main_menu.rs`)

- ASCII art btop logo (colored)
- Three selectable sections: Options, Help, Quit
- Arrow key / mouse navigation

#### 11.2 Options Menu (`menu/options_menu.rs`)

**5 categories (matching btop):**

| Category | # Options | Key options |
|---|---|---|
| 0: General | ~21 | color_theme, update_ms, graph_symbol, clock_format, rounded_corners, vim_keys, disable_mouse |
| 1: CPU | ~13 | cpu_bottom, cpu_graph_upper/lower, check_temp, show_coretemp, temp_scale, show_cpu_freq, show_uptime |
| 2: GPU | ~9 | graph_symbol_gpu, gpu_mirror_graph, custom_gpu_name0-5 |
| 3: Memory/Disk | ~13 | mem_below_net, show_disks, show_io_stat, io_mode, show_swap, only_physical, disks_filter |
| 4: Network | ~8 | graph_symbol_net, swap_upload_download, net_download/upload, net_auto, net_sync, net_iface |
| 5: Process | ~13 | proc_left, proc_sorting, proc_tree, proc_colors, proc_gradient, proc_per_core, proc_mem_bytes, proc_cpu_graphs |

**Option types:**
- Boolean toggle (True/False)
- String select (cycle through valid options)
- Integer input (with min/max validation)
- Text input (freeform)
- Theme selector (scrollable list)

#### 11.3 Help Menu (`menu/help_menu.rs`)

- 47+ keybind entries organized by category
- Scrollable if terminal too small
- Color-coded key names

#### 11.4 Process Action Menu (`menu/signal_menu.rs`)

Replaces btop's signal menu for Windows:

```
╭─ Process Action ───────╮
│                        │
│  [1] End Task          │
│  [2] Terminate         │
│  [3] Suspend           │
│  [4] Resume            │
│                        │
│  Enter: send  Esc: cancel │
╰────────────────────────╯
```

#### 11.5 Priority Menu (`menu/priority_menu.rs`)

```
╭─ Set Priority ─────────╮
│                         │
│  [1] Realtime    ▲▲▲   │
│  [2] High        ▲▲    │
│  [3] Above Normal ▲    │
│  [4] Normal      ═     │  ← current
│  [5] Below Normal ▼    │
│  [6] Idle        ▼▼    │
│                         │
│  Enter: set  Esc: cancel │
╰─────────────────────────╯
```

#### 11.6 Message Box (`menu/msg_box.rs`)

```rust
pub struct MsgBox {
    box_type: MsgBoxType,  // Ok, YesNo, NoYes
    content: Vec<String>,
    title: String,
}

pub enum MsgReturn {
    Invalid,
    OkYes,
    NoEsc,
    Select,
}
```

#### Tests (Phase 11)

```
test main_menu::render_logo
test main_menu::navigate_sections
test options_menu::render_general_category
test options_menu::navigate_categories
test options_menu::toggle_boolean_option
test options_menu::select_string_option
test options_menu::change_integer_option
test options_menu::theme_selector_scrolling
test help_menu::render_all_keybinds
test help_menu::scrolling
test signal_menu::render_actions
test signal_menu::select_action
test priority_menu::render_classes
test priority_menu::select_priority
test msg_box::render_ok
test msg_box::render_yes_no
test msg_box::input_enter_confirms
test msg_box::input_escape_cancels
```

---

### Phase 12: CLI Argument Parsing

**Goal:** Parse all command-line arguments matching btop's interface.

```rust
#[derive(Parser)]
#[command(name = "rtop", version, about = "Resource monitor for Windows")]
pub struct Cli {
    /// Path to config file
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<PathBuf>,

    /// Enable debug mode
    #[arg(short = 'd', long = "debug")]
    pub debug: bool,

    /// Set initial process filter
    #[arg(short = 'f', long = "filter")]
    pub filter: Option<String>,

    /// Disable truecolor (256 colors only)
    #[arg(short = 'l', long = "low-color")]
    pub low_color: bool,

    /// Start with preset (0-9)
    #[arg(short = 'p', long = "preset", value_parser = clap::value_parser!(u32).range(0..=9))]
    pub preset: Option<u32>,

    /// Force TTY mode
    #[arg(short = 't', long = "tty", conflicts_with = "no_tty")]
    pub tty: bool,

    /// Force disable TTY mode
    #[arg(long = "no-tty", conflicts_with = "tty")]
    pub no_tty: bool,

    /// Update rate in milliseconds (minimum 100)
    #[arg(short = 'u', long = "update", value_parser = clap::value_parser!(u32).range(100..))]
    pub update_ms: Option<u32>,

    /// Force UTF-8 output
    #[arg(long = "force-utf")]
    pub force_utf: bool,

    /// Custom themes directory
    #[arg(long = "themes-dir")]
    pub themes_dir: Option<PathBuf>,

    /// Print default config and exit
    #[arg(long = "default-config")]
    pub default_config: bool,
}
```

#### Tests (Phase 12)

```
test cli::parse_no_args
test cli::parse_config_file
test cli::parse_debug_flag
test cli::parse_filter
test cli::parse_preset_valid
test cli::parse_preset_out_of_range_error
test cli::parse_tty_and_no_tty_conflict
test cli::parse_update_ms_minimum
test cli::parse_themes_dir
test cli::parse_default_config
test cli::parse_version
test cli::parse_help
test cli::parse_short_flags
test cli::parse_long_flags
```

---

### Phase 13: Runner & Main Loop

**Goal:** Implement the background collection/drawing thread and the main event loop.

#### 13.1 Runner (`runner.rs`)

```rust
pub struct RunnerConfig {
    pub boxes: Vec<String>,
    pub no_update: bool,
    pub force_redraw: bool,
    pub background_update: bool,
    pub overlay: String,
    pub clock: String,
}

pub struct Runner {
    active: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    redraw: Arc<AtomicBool>,
    config: Mutex<RunnerConfig>,
    work_signal: Semaphore,
}

impl Runner {
    pub fn new() -> Self
    pub fn run(&self, box_name: &str, no_update: bool, force_redraw: bool)
    pub fn stop(&self)
    pub fn is_active(&self) -> bool
}
```

**Runner thread loop:**
1. Wait on semaphore
2. Acquire config lock
3. For each box in `boxes`:
   a. Collect data (calls `collect::*`)
   b. Draw to cell buffer (calls `ui::*`)
4. Apply overlay (menus) if active
5. Update clock display
6. Flush buffer to terminal
7. Release lock

#### 13.2 Main Entry (`main.rs`)

**Startup sequence:**
1. Parse CLI arguments
2. If `--default-config`: print and exit
3. Initialize logging
4. Initialize terminal (raw mode, alternate screen, mouse)
5. Load config file
6. Set theme
7. Initialize collectors (Shared::init)
8. Calculate initial layout
9. Start runner thread
10. Enter main event loop

**Main event loop:**
```rust
loop {
    // Check for terminal resize
    if terminal.refresh() {
        calc_sizes();
        runner.run("", false, true);  // Force redraw
    }

    // Check for config reload
    if reload_config.load(Ordering::Relaxed) {
        config::load(&config_path);
        theme::set_theme();
        runner.run("", false, true);
    }

    // Poll for input (with timeout = update_ms)
    if input::poll(config::get_int("update_ms") as u64) {
        if let Some(key) = input::get() {
            if menu::active() {
                menu::process(&key);
            } else {
                input::process(&key, &mut app_state);
            }
        }
    } else {
        // Timeout — trigger periodic update
        runner.run("", false, false);
    }

    // Check for quit
    if app_state.quitting {
        break;
    }
}
```

**Shutdown sequence:**
1. Stop runner thread
2. Restore terminal (normal screen, show cursor, mouse off)
3. Save config (if `save_config_on_exit`)
4. Flush logs
5. Exit

#### Tests (Phase 13)

```
test runner::run_triggers_collection
test runner::stop_halts_thread
test runner::force_redraw_redraws_all
test runner::no_update_skips_collection
test runner::concurrent_run_calls_safe
test main::startup_sequence_order
test main::shutdown_saves_config
test main::resize_triggers_recalculate
test main::input_routes_to_menu_when_active
test main::input_routes_to_process_when_normal
test main::periodic_update_on_timeout
```

---

### Phase 14: Theme Bundling & Banner

**Goal:** Embed all 40 btop themes and the ASCII art banner.

#### 14.1 Bundled Themes (40 files)

All themes embedded via `include_str!` macro:

```
HotPurpleTrafficLight, adapta, adwaita-dark, adwaita, ayu, dracula,
dusklight, elementarish, everforest-dark-hard, everforest-dark-medium,
everforest-light-medium, flat-remix-light, flat-remix, flexoki-dark,
flexoki-light, gotham, greyscale, gruvbox_dark, gruvbox_dark_v2,
gruvbox_light, gruvbox_material_dark, horizon, kanagawa-lotus,
kanagawa-wave, kyli0x, matcha-dark-sea, monokai, night-owl, nord,
onedark, orange, paper, phoenix-night, solarized_dark, solarized_light,
tokyo-night, tokyo-storm, tomorrow-night, twilight, whiteout
```

#### 14.2 Banner (`banner.rs`)

ASCII art logo matching btop's banner with color codes:

```
██████╗ ████████╗ ██████╗ ██████╗
██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
██████╔╝   ██║   ██║   ██║██████╔╝
██╔══██╗   ██║   ██║   ██║██╔═══╝
██║  ██║   ██║   ╚██████╔╝██║
╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝
```

#### Tests (Phase 14)

```
test banner::generate_centered
test banner::generate_with_position
test banner::color_codes_applied
test themes::all_bundled_themes_loadable
test themes::bundled_count_is_40
```

---

### Phase 15: Integration Testing & Polish

**Goal:** End-to-end testing, performance optimization, and edge case hardening.

#### 15.1 Integration Tests

```
test e2e::startup_and_quit
test e2e::resize_handling
test e2e::config_reload
test e2e::preset_cycling
test e2e::box_toggle_all_combinations
test e2e::process_filter_apply_and_clear
test e2e::process_tree_expand_collapse
test e2e::process_sort_all_columns
test e2e::network_interface_cycling
test e2e::theme_switching
test e2e::options_menu_navigation
test e2e::help_menu_display
test e2e::minimum_terminal_size_warning
```

#### 15.2 Performance Targets

| Metric | Target |
|---|---|
| Startup time | < 500ms |
| Update cycle (collection + render) | < 50ms |
| Input latency | < 16ms |
| Memory usage (idle) | < 20MB |
| Memory usage (1000 processes) | < 50MB |
| Binary size (release, stripped) | < 5MB |

#### 15.3 Edge Cases

- Terminal smaller than minimum size → show size error menu
- No network interfaces → show "No interfaces" in NET box
- Process handle access denied → show "(access denied)" for cmd/user
- GPU driver not installed → silently disable GPU boxes
- Battery not present → hide battery display
- Temperature unavailable → hide temperature display
- Config file corrupted → load defaults with warning
- Very high process count (>10,000) → ensure scrolling remains smooth
- Unicode process names (CJK, emoji) → correct column alignment
- Very long command lines → truncation with ellipsis
- Rapid terminal resize → debounce recalculation
- System sleep/resume → reconnect collectors

---

## 6. Dependency Graph (Phase Ordering)

```
Phase 0: Scaffolding
    │
    ▼
Phase 1: Domain Types ──────────────────────────┐
    │                                            │
    ▼                                            │
Phase 2: Tools & Logging                        │
    │                                            │
    ▼                                            │
Phase 3: Config ─────────────────┐              │
    │                            │              │
    ▼                            │              │
Phase 4: Theme ◄─────────────────┘              │
    │                                            │
    ▼                                            │
Phase 5: Cell Buffer                             │
    │                                            │
    ▼                                            │
Phase 6: Terminal Backend                        │
    │                                            │
    ▼                                            │
Phase 7: Input System                            │
    │                                            │
    ▼                                            │
Phase 8: Drawing Primitives                      │
    │                                            │
    ├──────────────────┐                         │
    ▼                  ▼                         │
Phase 9: Collectors   Phase 10: UI Boxes ◄───────┘
    │                  │
    ▼                  ▼
Phase 11: Menus ◄──────┘
    │
    ▼
Phase 12: CLI
    │
    ▼
Phase 13: Runner & Main Loop
    │
    ▼
Phase 14: Theme Bundling & Banner
    │
    ▼
Phase 15: Integration & Polish
```

---

## 7. File Counts & Scope Estimates

| Phase | Files | Approximate Lines |
|---|---|---|
| Phase 0: Scaffolding | 5 | 200 |
| Phase 1: Domain Types | 6 | 800 |
| Phase 2: Tools & Logging | 2 | 1,200 |
| Phase 3: Config | 1 | 1,500 |
| Phase 4: Theme | 1 | 1,200 |
| Phase 5: Cell Buffer | 1 | 800 |
| Phase 6: Terminal Backend | 1 | 600 |
| Phase 7: Input System | 1 | 1,000 |
| Phase 8: Drawing Primitives | 5 | 2,500 |
| Phase 9: Collectors | 6 | 4,000 |
| Phase 10: UI Boxes | 5 | 5,000 |
| Phase 11: Menus | 6 | 2,500 |
| Phase 12: CLI | 1 | 200 |
| Phase 13: Runner & Main | 2 | 800 |
| Phase 14: Bundling & Banner | 1 | 300 |
| Phase 15: Integration | 1 | 500 |
| **Tests** | ~30 | **~5,000** |
| **Total** | **~75** | **~28,000** |

---

## 8. Windows-Specific Design Decisions

### 8.1 Process Actions (Replacing Unix Signals)

btop's signal menu offers 31 Unix signals. On Windows, we replace this with 4 meaningful actions:

| Action | API | Description |
|---|---|---|
| **End Task** | `PostMessageW(WM_CLOSE)` via `EnumWindows` | Gracefully asks process to close (equivalent to SIGTERM) |
| **Terminate** | `TerminateProcess(handle, 1)` | Forcefully kills process (equivalent to SIGKILL) |
| **Suspend** | `NtSuspendProcess(handle)` | Pauses all threads (equivalent to SIGSTOP) |
| **Resume** | `NtResumeProcess(handle)` | Resumes all threads (equivalent to SIGCONT) |

### 8.2 Process Priority (Replacing nice)

btop uses Unix nice values (-20 to 19). On Windows, we map to 6 priority classes:

| Priority Class | SetPriorityClass constant | Display |
|---|---|---|
| Realtime | `REALTIME_PRIORITY_CLASS` | `▲▲▲` |
| High | `HIGH_PRIORITY_CLASS` | `▲▲` |
| Above Normal | `ABOVE_NORMAL_PRIORITY_CLASS` | `▲` |
| Normal | `NORMAL_PRIORITY_CLASS` | `═` |
| Below Normal | `BELOW_NORMAL_PRIORITY_CLASS` | `▼` |
| Idle | `IDLE_PRIORITY_CLASS` | `▼▼` |

### 8.3 Process States

| Windows State | Display | Detection |
|---|---|---|
| Running | "Running" | Process exists and not suspended |
| Suspended | "Suspended" | All threads are in suspended state (via `NtQuerySystemInformation`) |
| Not Responding | "Not Responding" | `IsHungAppWindow()` for GUI processes |
| Unknown | "Unknown" | Default fallback |

### 8.4 Temperature Sensors — Runtime Detection Chain

Temperature is the hardest metric on Windows. We detect available sources **at runtime** during startup (in `Shared::init()`) with a priority-ordered fallback chain. No hard dependency on any third-party tool — if nothing is available, the UI degrades gracefully.

**Detection chain (checked in order, first match wins):**

| Priority | Source | WMI Namespace / API | What You Get | Requirement |
|---|---|---|---|---|
| 1 | LibreHardwareMonitor | `root/LibreHardwareMonitor` → `Sensor WHERE SensorType='Temperature'` | Per-core CPU temp, package temp, GPU temp (all vendors), CPU watts, fan speeds | LHM service running |
| 2 | OpenHardwareMonitor | `root/OpenHardwareMonitor` → `Sensor WHERE SensorType='Temperature'` | Same as LHM (legacy format) | OHM service running |
| 3 | WMI Thermal Zone | `root/WMI` → `MSAcpi_ThermalZoneTemperature` | Zone-level temp only (not per-core), often inaccurate | Admin privileges |
| 4 | None | — | Temperature display hidden entirely | — |

**Implementation:**
```rust
pub enum TempSource {
    LibreHardwareMonitor,   // Best: per-core, watts, GPU, fans
    OpenHardwareMonitor,    // Good: same data, legacy app
    WmiThermalZone,         // Minimal: zone-level only, admin required
    None,                   // No source — hide temp UI
}

fn detect_temperature_source() -> TempSource {
    // Try LHM first (non-blocking WMI query, <50ms)
    if wmi_namespace_exists("root/LibreHardwareMonitor") { return TempSource::LibreHardwareMonitor; }
    if wmi_namespace_exists("root/OpenHardwareMonitor") { return TempSource::OpenHardwareMonitor; }
    if is_elevated() && wmi_thermal_zone_works() { return TempSource::WmiThermalZone; }
    TempSource::None
}
```

**UI behavior when `TempSource::None`:**
- CPU box: temperature column and per-core temp graphs are hidden (space reclaimed by other elements)
- Options menu: `check_temp` shows "(no sensor found — install LibreHardwareMonitor for CPU temperature)"
- `show_cpu_watts` shows "N/A"
- `temp_scale` option still configurable (takes effect if source becomes available on config reload)
- Log: `INFO "No temperature sensor source detected. Install LibreHardwareMonitor service for CPU/GPU temperature monitoring."`

### 8.5 Config File Path

```
%APPDATA%\rtop\rtop.conf        # Config file
%APPDATA%\rtop\themes\          # User themes directory
%LOCALAPPDATA%\rtop\rtop.log    # Log file
```

### 8.6 Linux-Only Config Keys (Handled on Windows)

| Key | Windows Behavior |
|---|---|
| `use_fstab` | Ignored (no fstab on Windows) |
| `zfs_arc_cached` | Ignored (no ZFS on Windows) |
| `zfs_hide_datasets` | Ignored |
| `proc_info_smaps` | Ignored (no smaps; always use working set) |
| `disk_free_priv` | Ignored (Windows always shows available to user) |
| `proc_filter_kernel` | Repurposed: filter Session 0 / system processes |
| `freq_mode` | Supported: first, highest, lowest, average, range |

---

## 9. Test-Driven Development Workflow

### 9.1 TDD Cycle for Each Module

```
1. Write test file: src/module_test.rs (or tests/module.rs)
2. Define test cases covering:
   - Happy path (expected input → expected output)
   - Edge cases (empty, max, min, boundary values)
   - Error cases (invalid input, missing data, permission denied)
   - Property tests where applicable (layout calculations)
3. Run `cargo test` — all new tests FAIL (Red)
4. Implement minimum code to make tests pass (Green)
5. Refactor for clarity and performance (Refactor)
6. Run `cargo clippy` — no warnings
7. Run `cargo fmt` — clean formatting
8. Commit with descriptive message
```

### 9.2 Test Categories

| Category | Tool | Location |
|---|---|---|
| Unit tests | `#[cfg(test)] mod tests` | Inline in each module |
| Snapshot tests | `insta` crate | `tests/snapshots/` |
| Property tests | `proptest` crate | Inline or `tests/props/` |
| Integration tests | Standard | `tests/` |
| Benchmark tests | `criterion` | `benches/` |
| Fixture tests | Custom | `tests/fixtures/` |

### 9.3 Test Data Fixtures

Create `tests/fixtures/` directory with:
- `default.conf` — default config file
- `custom.conf` — config with non-default values
- `invalid.conf` — config with invalid values
- `dracula.theme` — sample theme file
- `cpu_snapshot.json` — captured CPU performance data
- `process_list.json` — captured process list
- `network_adapters.json` — captured network adapter info

### 9.4 Continuous Integration

```yaml
# .github/workflows/ci.yml
on: [push, pull_request]
jobs:
  test:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all-features
      - run: cargo clippy --all-features -- -D warnings
      - run: cargo fmt -- --check

  test-no-gpu:
    runs-on: windows-latest
    steps:
      - run: cargo test  # Without gpu feature

  benchmark:
    runs-on: windows-latest
    steps:
      - run: cargo bench --bench rendering
```

---

## 10. Summary

This plan provides a complete roadmap for recreating btop in Rust for Windows 11 with:

- **15 implementation phases** following strict TDD methodology
- **~75 source files** organized into clear module boundaries
- **~28,000 lines of Rust** (including ~5,000 lines of tests)
- **Complete feature parity** with btop v1.4.6 including all 100+ config options, 40 themes, 50+ keybinds, 5 UI boxes, 8 menu types, 3 graph symbol modes, and 4 temperature scales
- **Windows-native design decisions** for signals→process actions, nice→priority classes, /proc→Win32 API, and temperature sensor strategies
- **Off-screen cell buffer architecture** enabling deterministic snapshot testing without a real terminal
- **Graceful degradation** for features that cannot fully map to Windows (temperatures, GPU metrics, process details)
- **Performance targets** of <50ms update cycles and <20MB memory usage

The domain model layer ensures clean separation between OS-specific collectors and the rendering system, making future platform ports feasible while keeping the Windows implementation idiomatic and performant.
