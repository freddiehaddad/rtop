use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::{CpuGraphSource, GraphSymbol, TempScale};
use crate::domain::layout_spec::Slot;
use crate::domain::preset::CustomLayout;
use crate::domain::widget_kind::WidgetKind;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing_subscriber::filter::LevelFilter;

/// Maximum number of GPUs supported.
pub const MAX_GPUS: usize = 8;

/// Error returned by `ConfigKey::set_string` when the supplied
/// string cannot be parsed into the field's value type.
///
/// The single in-tree caller (`menu::options_menu::cycle_browsable`)
/// always passes a value drawn from `browsable_values()`, so this
/// error is unreachable at runtime and the caller `.expect()`s it.
/// Carrying the key name and offending value makes the panic
/// message informative if the contract is ever violated.
#[derive(Debug, Error)]
#[error("invalid value '{value}' for config key '{key}'")]
pub struct SetStringError {
    pub key: &'static str,
    pub value: String,
}

/// The kind of a config key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Bool,
    Int,
    String,
    /// A field backed by a typed enum with a closed set of
    /// canonical names. Treated by the menu exactly like
    /// `String` with `browsable_values`, but the underlying
    /// Config field is the enum type itself.
    Enum,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn clamp_warn(field: &mut i64, min: i64, max: i64, name: &str, warnings: &mut Vec<String>) {
    let old = *field;
    *field = old.clamp(min, max);
    if old != *field {
        warnings.push(format!(
            "Value for '{name}' out of range ({old}), clamped to {}",
            *field
        ));
    }
}

fn validate_choice(
    field: &mut String,
    default: &str,
    choices: &[&str],
    name: &str,
    warnings: &mut Vec<String>,
) {
    if !choices.contains(&field.as_str()) {
        warnings.push(format!(
            "Invalid value for '{name}': '{}' (expected one of: {})",
            field,
            choices.join(", ")
        ));
        *field = default.to_string();
    }
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// All configuration state for rtop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // -- bools --
    pub theme_background: bool,
    pub rounded_corners: bool,
    pub proc_reversed: bool,
    pub proc_tree: bool,
    pub proc_colors: bool,
    pub proc_gradient: bool,
    pub proc_per_core: bool,
    pub proc_mem_bytes: bool,

    pub proc_aggregate: bool,
    pub keep_dead_proc_usage: bool,
    pub cpu_invert_lower: bool,
    pub cpu_single_graph: bool,
    pub cpu_auto_scale: bool,
    pub show_uptime: bool,
    pub show_cpu_watts: bool,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub show_cpu_freq: bool,
    pub show_swap: bool,

    pub show_io_stat: bool,
    pub io_mode: bool,
    pub io_graph_combined: bool,
    pub swap_upload_download: bool,
    pub base_10_sizes: bool,
    pub net_auto: bool,
    pub net_sync: bool,

    pub vim_keys: bool,
    pub background_update: bool,
    pub terminal_sync: bool,
    pub save_config_on_exit: bool,

    pub disk_io_mode: bool,

    // -- ints --
    pub update_ms: i64,
    pub cpu_update_ms: i64,
    pub mem_update_ms: i64,
    pub disk_update_ms: i64,
    pub net_update_ms: i64,
    pub gpu_update_ms: i64,
    pub proc_update_ms: i64,
    pub net_download: i64,
    pub net_upload: i64,

    pub preset: crate::domain::preset::PresetField,

    /// Persisted layout for the `Custom` preset slot. Serialised
    /// as a TOML `[layout]` table.
    #[serde(rename = "layout")]
    pub custom: CustomLayout,

    // -- strings --
    pub color_theme: String,
    pub graph_symbol: GraphSymbol,
    pub graph_symbol_cpu: GraphSymbol,
    pub graph_symbol_net: GraphSymbol,
    pub graph_symbol_disk: GraphSymbol,
    pub proc_sorting: ProcSort,
    pub cpu_graph_upper: CpuGraphSource,
    pub cpu_graph_lower: CpuGraphSource,

    pub temp_scale: TempScale,
    pub clock_format: String,
    pub custom_cpu_name: String,
    pub disks_filter: Vec<String>,
    pub net_iface: String,
    #[serde(
        with = "crate::log::serde_filter",
        default = "crate::log::default_filter"
    )]
    pub log_level: LevelFilter,
    pub proc_filter: String,
    pub custom_gpu_names: [String; MAX_GPUS],

    #[serde(skip)]
    conf_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // bools
            theme_background: true,
            rounded_corners: true,
            proc_reversed: false,
            proc_tree: false,
            proc_colors: true,
            proc_gradient: true,
            proc_per_core: false,
            proc_mem_bytes: true,

            proc_aggregate: false,
            keep_dead_proc_usage: false,
            cpu_invert_lower: true,
            cpu_single_graph: false,
            cpu_auto_scale: false,
            show_uptime: true,
            show_cpu_watts: true,
            check_temp: true,
            show_coretemp: true,
            show_cpu_freq: true,
            show_swap: true,

            show_io_stat: true,
            io_mode: false,
            io_graph_combined: false,
            swap_upload_download: false,
            base_10_sizes: false,
            net_auto: true,
            net_sync: false,

            vim_keys: false,
            background_update: true,
            terminal_sync: true,
            save_config_on_exit: true,

            disk_io_mode: false,

            // ints
            update_ms: 2000,
            cpu_update_ms: 0,
            mem_update_ms: 0,
            disk_update_ms: 0,
            net_update_ms: 0,
            gpu_update_ms: 0,
            proc_update_ms: 0,
            net_download: 100,
            net_upload: 100,

            // Default to the custom preset so first-launch users
            // see what their TOML actually says (the `[layout]`
            // block below). Otherwise the cursor would lie.
            preset: crate::domain::preset::PresetField::new(
                crate::domain::preset::ActivePreset::Custom,
            ),

            // First-launch custom layout: every widget rtop knows
            // about (see `CustomLayout::default`). New GPUs plugged
            // in later are picked up automatically because their
            // index is already in the list.
            custom: CustomLayout::default(),

            // strings
            color_theme: "default".to_string(),
            graph_symbol: GraphSymbol::Braille,
            graph_symbol_cpu: GraphSymbol::Default,
            graph_symbol_net: GraphSymbol::Default,
            graph_symbol_disk: GraphSymbol::Default,
            proc_sorting: ProcSort::Cpu,
            cpu_graph_upper: CpuGraphSource::User,
            cpu_graph_lower: CpuGraphSource::System,

            temp_scale: TempScale::Celsius,
            clock_format: "%X".to_string(),
            custom_cpu_name: String::new(),
            disks_filter: Vec::new(),
            net_iface: "auto".to_string(),
            log_level: LevelFilter::WARN,
            proc_filter: String::new(),
            custom_gpu_names: Default::default(),

            conf_file: None,
        }
    }
}

impl Config {
    /// Create a new Config with all default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the effective update interval for a per-widget field.
    ///
    /// If `widget_ms` is 0 (the default), returns the global `update_ms`.
    /// Otherwise returns `widget_ms` clamped to [100, 86_400_000].
    pub fn effective_interval(&self, widget_ms: i64) -> u64 {
        if widget_ms > 0 {
            (widget_ms.clamp(100, 86_400_000)) as u64
        } else {
            self.update_ms.max(100) as u64
        }
    }

    /// Load config from a TOML file. Returns warnings for invalid values.
    pub fn load(&mut self, path: &Path) -> Vec<String> {
        let mut warnings = Vec::new();
        self.conf_file = Some(path.to_path_buf());

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return warnings,
        };

        if content.trim().is_empty() {
            return warnings;
        }

        // Preserve runtime-only fields across the load.
        let saved_conf = self.conf_file.take();

        match toml::from_str::<Config>(&content) {
            Ok(loaded) => {
                *self = loaded;
            }
            Err(e) => {
                warnings.push(format!("Failed to parse config: {e}"));
                // Restore runtime fields and return early.
                self.conf_file = saved_conf;
                return warnings;
            }
        }

        self.conf_file = saved_conf;

        warnings.append(&mut self.validate());
        warnings
    }

    /// Write config to a TOML file.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let content =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    /// Validate all config values, clamping ints and resetting invalid strings.
    pub fn validate(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Clamp integers
        clamp_warn(
            &mut self.update_ms,
            100,
            86_400_000,
            "update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.cpu_update_ms,
            0,
            86_400_000,
            "cpu_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.mem_update_ms,
            0,
            86_400_000,
            "mem_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.disk_update_ms,
            0,
            86_400_000,
            "disk_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_update_ms,
            0,
            86_400_000,
            "net_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.gpu_update_ms,
            0,
            86_400_000,
            "gpu_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.proc_update_ms,
            0,
            86_400_000,
            "proc_update_ms",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_download,
            0,
            10_000_000,
            "net_download",
            &mut warnings,
        );
        clamp_warn(
            &mut self.net_upload,
            0,
            10_000_000,
            "net_upload",
            &mut warnings,
        );

        // The only remaining `validate_choice` call covers
        // `color_theme` — the single string-typed field with a
        // closed set of choices. The other browsable config
        // fields are typed enums whose validity is enforced by
        // serde at deserialise time.
        validate_choice(
            &mut self.color_theme,
            "default",
            crate::theme::THEME_NAMES,
            "color_theme",
            &mut warnings,
        );

        // Surface deserialise-time parse failures from the active
        // preset cursor. Bad names are already dropped by
        // PresetField's deserialiser (defaulting to Custom); we
        // just need to fold the captured offending name into the
        // warning list.
        if let Some(invalid) = self.preset.take_invalid() {
            warnings.push(format!(
                "Unknown preset name in 'preset': '{invalid}', falling back to custom",
            ));
        }

        // Validate disks_filter: surface invalid drive entries as
        // warnings and drop them in place so the saved config
        // matches what the runtime actually uses.
        let filter = crate::domain::disk::DisksFilter::parse(&self.disks_filter);
        if !filter.invalid().is_empty() {
            warnings.push(format!(
                "Invalid drive entry/entries in 'disks_filter': {}",
                filter.invalid().join(", ")
            ));
            let invalid_tokens = filter.invalid().to_vec();
            self.disks_filter
                .retain(|tok| !invalid_tokens.contains(tok));
        }

        warnings
    }

    /// Reload config from the stored config file path.
    pub fn reload(&mut self) -> Vec<String> {
        if let Some(path) = self.conf_file.clone() {
            self.apply_defaults();
            self.load(&path)
        } else {
            Vec::new()
        }
    }

    /// Reset to defaults, preserving runtime-only fields.
    pub fn apply_defaults(&mut self) {
        let saved_conf = self.conf_file.take();
        *self = Self::default();
        self.conf_file = saved_conf;
    }

    /// Build the canonical [`Slot`] tree for the active preset.
    ///
    /// Builtins return a static tree from
    /// [`crate::domain::preset::BuiltinPreset::layout_spec`]; the
    /// custom preset returns a clone of its persisted root tree.
    pub fn layout_spec(&self) -> Slot {
        match self.preset.active() {
            crate::domain::preset::ActivePreset::Builtin(b) => b.layout_spec(),
            crate::domain::preset::ActivePreset::Custom => self.custom.layout_spec(),
        }
    }

    /// Move the preset cursor one position in the cycle. `forward`
    /// = `true` advances (`p`); `forward` = `false` retreats (`P`).
    /// The cycle wraps over [`crate::domain::preset::ActivePreset::CYCLE_LEN`]
    /// positions (the four builtins plus custom).
    pub fn cycle_preset(&mut self, forward: bool) {
        let next = if forward {
            self.preset.active().next()
        } else {
            self.preset.active().prev()
        };
        self.preset.set(next);
    }

    /// Toggle a single widget on or off in the custom preset.
    /// Auto-promotes from a builtin preset to the custom one
    /// (copying the active builtin's tree into custom first) so the
    /// user's edit always lands somewhere persistent.
    ///
    /// Toggling **off** the only visible widget is a no-op — every
    /// custom layout must contain at least one widget; users who want
    /// a different layout entirely should edit `shape` directly.
    pub fn toggle_widget(&mut self, kind: WidgetKind) {
        let custom = self.promote_to_custom();
        if custom.root.contains(kind) {
            // Remove. Skip if pruning would empty the tree.
            if let Some(pruned) = custom.root.clone().pruned(kind) {
                custom.root = pruned;
            }
        } else {
            // Add. `append_widget` no-ops if the widget is already
            // present (defensive — `contains` already gated us).
            custom.root = custom.root.clone().append_widget(kind);
        }
    }

    /// Replace the live layout's [`Slot`] tree with one parsed from
    /// the DSL string `value`.
    ///
    /// No-op if the parsed tree equals the live tree — preserves the
    /// cursor on a builtin so an "edit" that doesn't actually change
    /// anything doesn't surprise-promote. Otherwise promotes-to-custom
    /// and writes the new tree.
    ///
    /// Returns `Err` if the DSL fails to parse or fails post-parse
    /// validation (duplicate widget kinds, etc.).
    pub fn set_shape(
        &mut self,
        value: &str,
    ) -> Result<(), crate::domain::layout_spec::SlotParseError> {
        let next: Slot = value.parse()?;
        if self.layout_spec() == next {
            return Ok(());
        }
        self.promote_to_custom().root = next;
        Ok(())
    }

    /// Move the cursor to `Custom`, copying the active builtin's
    /// [`Slot`] tree into `self.custom` on first promotion, and return
    /// a mutable borrow of the custom slot. Already-on-Custom is a
    /// no-op (no copy). Used by every layout-mutating operation so
    /// the user's edit always lands in the persistent slot.
    fn promote_to_custom(&mut self) -> &mut CustomLayout {
        if let crate::domain::preset::ActivePreset::Builtin(b) = self.preset.active() {
            self.custom = CustomLayout {
                root: b.layout_spec(),
            };
            self.preset.set(crate::domain::preset::ActivePreset::Custom);
        }
        &mut self.custom
    }
}

// ---------------------------------------------------------------------------
// ConfigKey — declarative schema
// ---------------------------------------------------------------------------
//
// `ConfigKey` is a flat enum identifying every config field. The
// previous shape of this file kept eight parallel hand-maintained
// match arms (`name`, `kind`, `parse`, `get_display`, `toggle_bool`,
// `get_int`, `set_int`, plus the enum itself). Adding or renaming a
// key meant editing every one of them in lockstep.
//
// `config_schema!` collapses those eight mirrors into one declarative
// table. Each entry binds:
//   * the enum variant
//   * the TOML/snake_case name
//   * the value kind (`Bool` / `Int` / `Enum` / `String`)
//   * the access shape (how the value is read/written on `Config`)
//
// Access shapes:
//   * `field $f`                 — direct `config.$f`
//   * `joined_vec $f`            — `Vec<String>` displayed as a
//                                  whitespace-joined string
//   * `array $f [$i]`            — fixed-array element at index `$i`
//   * `shape`                    — the `shape` virtual key, derived
//                                  from `config.layout_spec()` and
//                                  written via `config.set_shape()`
//
// The macro emits the enum and the eight previously hand-mirrored
// methods. `set_string`, `validate_string`, `parse_int`,
// `int_bounds_message`, `parse_disks_filter`, and `choice_values`
// stay hand-written below — they carry per-shape error handling and
// validation logic that does not benefit from being declarative.
//
// `Config`, `Default for Config`, and `validate()` are also
// hand-written; the struct field list carries serde attributes
// (`#[serde(rename = "...")]`, `#[serde(with = "...")]`, `#[serde(skip)]`)
// and the existing schema includes one virtual key (`Shape`) without
// a 1:1 struct field. Folding that into the macro would cost more
// clarity than it would save.

macro_rules! config_schema {
    (
        $(
            $variant:ident => $name:literal : $kind:ident { $($shape:tt)* }
            [ $($desc:literal),* $(,)? ]
        ),* $(,)?
    ) => {
        /// A flat enum identifying every config field.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ConfigKey { $( $variant ),* }

        impl ConfigKey {
            /// Returns the TOML field name (snake_case).
            pub fn name(self) -> &'static str {
                match self { $( Self::$variant => $name, )* }
            }

            /// Returns the kind of value this key holds.
            pub fn kind(self) -> KeyKind {
                match self { $( Self::$variant => KeyKind::$kind, )* }
            }

            /// Returns the multi-line help text shown for this option
            /// when it is focused in the options menu.
            ///
            /// Each entry is one rendered line; an empty string is a
            /// blank separator line.
            pub fn desc(self) -> &'static [&'static str] {
                match self {
                    $( Self::$variant => &[ $( $desc ),* ], )*
                }
            }

            /// Parse a TOML field name into a `ConfigKey`.
            #[cfg(test)]
            pub fn parse(name: &str) -> Option<Self> {
                match name {
                    $( $name => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// Returns a display string for the value of this key in
            /// the given config.
            pub fn get_display(self, config: &Config) -> String {
                match self {
                    $( Self::$variant => config_schema!(@display $kind config $($shape)*), )*
                }
            }

            /// Toggle the boolean field.
            ///
            /// Panics on non-bool keys: this is a programmer error
            /// (the caller must dispatch on `kind()` first).
            pub fn toggle_bool(self, config: &mut Config) {
                match self {
                    $( Self::$variant => config_schema!(@toggle self $kind config $($shape)*), )*
                }
            }

            /// Get an integer value.
            ///
            /// Panics on non-int keys: this is a programmer error
            /// (the caller must dispatch on `kind()` first).
            pub fn get_int(self, config: &Config) -> i64 {
                match self {
                    $( Self::$variant => config_schema!(@get_int self $kind config $($shape)*), )*
                }
            }

            /// Set an integer value. No clamping — caller should call
            /// `validate()`.
            ///
            /// Panics on non-int keys: this is a programmer error
            /// (the caller must dispatch on `kind()` first).
            pub fn set_int(self, config: &mut Config, value: i64) {
                match self {
                    $( Self::$variant => config_schema!(@set_int self $kind config value $($shape)*), )*
                }
            }
        }
    };

    // === @display: kind config <shape> -> String ===
    (@display Bool $cfg:ident field $f:ident) => { bool_display($cfg.$f) };
    (@display Int $cfg:ident field $f:ident) => { $cfg.$f.to_string() };
    (@display Enum $cfg:ident field $f:ident) => { $cfg.$f.to_string() };
    (@display String $cfg:ident field $f:ident) => { $cfg.$f.clone() };
    (@display String $cfg:ident joined_vec $f:ident) => { $cfg.$f.join(" ") };
    (@display String $cfg:ident array $f:ident [$i:literal]) => { $cfg.$f[$i].clone() };
    (@display String $cfg:ident shape) => { $cfg.layout_spec().to_string() };

    // === @toggle: only Bool variants do anything; others panic ===
    (@toggle $self:ident Bool $cfg:ident field $f:ident) => { $cfg.$f = !$cfg.$f };
    (@toggle $self:ident Int $cfg:ident $($_:tt)*) => {
        panic!("toggle_bool called on non-bool key '{}'", $self.name())
    };
    (@toggle $self:ident Enum $cfg:ident $($_:tt)*) => {
        panic!("toggle_bool called on non-bool key '{}'", $self.name())
    };
    (@toggle $self:ident String $cfg:ident $($_:tt)*) => {
        panic!("toggle_bool called on non-bool key '{}'", $self.name())
    };

    // === @get_int: only Int kind returns a value; others panic ===
    (@get_int $self:ident Int $cfg:ident field $f:ident) => { $cfg.$f };
    (@get_int $self:ident Bool $cfg:ident $($_:tt)*) => {
        panic!("get_int called on non-int key '{}'", $self.name())
    };
    (@get_int $self:ident Enum $cfg:ident $($_:tt)*) => {
        panic!("get_int called on non-int key '{}'", $self.name())
    };
    (@get_int $self:ident String $cfg:ident $($_:tt)*) => {
        panic!("get_int called on non-int key '{}'", $self.name())
    };

    // === @set_int: only Int kind writes; others panic ===
    (@set_int $self:ident Int $cfg:ident $val:ident field $f:ident) => { $cfg.$f = $val };
    (@set_int $self:ident Bool $cfg:ident $val:ident $($_:tt)*) => {
        panic!("set_int called on non-int key '{}'", $self.name())
    };
    (@set_int $self:ident Enum $cfg:ident $val:ident $($_:tt)*) => {
        panic!("set_int called on non-int key '{}'", $self.name())
    };
    (@set_int $self:ident String $cfg:ident $val:ident $($_:tt)*) => {
        panic!("set_int called on non-int key '{}'", $self.name())
    };
}

config_schema! {
    // -- bool, direct field --
    ThemeBackground => "theme_background" : Bool { field theme_background } [
        "Theme background color.",
        "",
        "Set to False for terminal background",
        "transparency.",
    ],
    RoundedCorners => "rounded_corners" : Bool { field rounded_corners } [
        "Rounded corners on widgets.",
        "",
        "True or False.",
    ],
    ProcReversed => "proc_reversed" : Bool { field proc_reversed } [
        "Reverse sort order.",
        "",
        "True or False.",
    ],
    ProcTree => "proc_tree" : Bool { field proc_tree } [
        "Tree view.",
        "",
        "Group processes by parent with",
        "lines drawn between parent and",
        "child processes.",
    ],
    ProcColors => "proc_colors" : Bool { field proc_colors } [
        "Process row colors.",
        "",
        "Color process rows based on",
        "CPU usage.",
    ],
    ProcGradient => "proc_gradient" : Bool { field proc_gradient } [
        "Process color gradient.",
        "",
        "Fade row colors based on distance",
        "from the selected process.",
    ],
    ProcPerCore => "proc_per_core" : Bool { field proc_per_core } [
        "Per-core CPU usage.",
        "",
        "Show CPU usage relative to one",
        "core instead of total CPU power.",
        "Values can exceed 100%.",
    ],
    ProcMemBytes => "proc_mem_bytes" : Bool { field proc_mem_bytes } [
        "Memory as bytes.",
        "",
        "Show memory in bytes instead of",
        "percentage of total memory.",
    ],
    ProcAggregate => "proc_aggregate" : Bool { field proc_aggregate } [
        "Aggregate child resources.",
        "",
        "In tree view, include child CPU",
        "and memory usage in the parent",
        "process totals.",
    ],
    KeepDeadProcUsage => "keep_dead_proc_usage" : Bool { field keep_dead_proc_usage } [
        "Preserve dead process usage.",
        "",
        "Keep CPU and memory values for",
        "processes that have exited.",
    ],
    CpuInvertLower => "cpu_invert_lower" : Bool { field cpu_invert_lower } [
        "Invert lower CPU graph.",
        "",
        "Flips the orientation of the lower",
        "CPU graph so it grows downward.",
    ],
    CpuSingleGraph => "cpu_single_graph" : Bool { field cpu_single_graph } [
        "Single CPU graph.",
        "",
        "Disable the lower CPU graph and",
        "expand the upper graph to full",
        "widget height.",
    ],
    CpuAutoScale => "cpu_auto_scale" : Bool { field cpu_auto_scale } [
        "Auto-scale CPU graph y-axis.",
        "",
        "Off: graph height maps to absolute",
        "0-100% (default).",
        "On: scale to the largest visible",
        "value (recolours by visible max,",
        "not absolute %).",
    ],
    ShowUptime => "show_uptime" : Bool { field show_uptime } [
        "System uptime display.",
        "",
        "Show system uptime in the CPU widget.",
    ],
    ShowCpuWatts => "show_cpu_watts" : Bool { field show_cpu_watts } [
        "CPU power consumption.",
        "",
        "Show wattage in the CPU widget.",
        "Requires LibreHardwareMonitor.",
    ],
    CheckTemp => "check_temp" : Bool { field check_temp } [
        "CPU temperature monitoring.",
        "",
        "Enable temperature reporting in",
        "the CPU widget.",
    ],
    ShowCoretemp => "show_coretemp" : Bool { field show_coretemp } [
        "Per-core temperatures.",
        "",
        "Show individual core temperatures.",
        "Requires temperature monitoring",
        "to be enabled.",
    ],
    ShowCpuFreq => "show_cpu_freq" : Bool { field show_cpu_freq } [
        "CPU frequency display.",
        "",
        "Show the current CPU clock speed",
        "in the core panel.",
    ],
    ShowSwap => "show_swap" : Bool { field show_swap } [
        "Swap memory display.",
        "",
        "Show swap usage in the memory widget.",
    ],
    ShowIoStat => "show_io_stat" : Bool { field show_io_stat } [
        "Disk IO activity indicators.",
        "",
        "Show read/write throughput data",
        "alongside disk usage meters.",
    ],
    IoMode => "io_mode" : Bool { field io_mode } [
        "IO mode toggle.",
        "",
        "Switch between usage meters and",
        "IO throughput graphs with the",
        "\"i\" key.",
    ],
    IoGraphCombined => "io_graph_combined" : Bool { field io_graph_combined } [
        "Combined IO graph.",
        "",
        "Merge read and write into a single",
        "graph. Only applies in IO mode.",
    ],
    SwapUploadDownload => "swap_upload_download" : Bool { field swap_upload_download } [
        "Swap upload and download positions.",
    ],
    Base10Sizes => "base_10_sizes" : Bool { field base_10_sizes } [
        "Base 10 size units.",
        "",
        "Uses KB = 1000 instead of",
        "KiB = 1024.",
    ],
    NetAuto => "net_auto" : Bool { field net_auto } [
        "Auto scale network graphs.",
        "",
        "Automatically adjust graph scale",
        "based on current traffic.",
    ],
    NetSync => "net_sync" : Bool { field net_sync } [
        "Sync network graph scales.",
        "",
        "Use the same scale for both upload",
        "and download graphs.",
    ],
    VimKeys => "vim_keys" : Bool { field vim_keys } [
        "Vim key bindings.",
        "",
        "h/j/k/l for directional control,",
        "g/G for top/bottom of list,",
        "Ctrl+F/B/D/U for page scrolling.",
    ],
    BackgroundUpdate => "background_update" : Bool { field background_update } [
        "Update while menus are open.",
        "",
        "Continue refreshing data when the",
        "options or help menu is visible.",
    ],
    TerminalSync => "terminal_sync" : Bool { field terminal_sync } [
        "Terminal output synchronization.",
        "",
        "Reduces flickering on supported",
        "terminals.",
    ],
    SaveConfigOnExit => "save_config_on_exit" : Bool { field save_config_on_exit } [
        "Save settings on exit.",
        "",
        "Automatically write current settings",
        "to the config file on exit.",
    ],
    DiskIoMode => "disk_io_mode" : Bool { field disk_io_mode } [
        "Persistent IO mode.",
        "",
        "Always show IO throughput graphs",
        "instead of usage meters.",
    ],

    // -- int, direct field --
    UpdateMs => "update_ms" : Int { field update_ms } [
        "Update interval in milliseconds.",
        "",
        "Recommended 2000 ms or above for",
        "better graph sample times.",
        "",
        "Range: 100 ms to 86400000 ms.",
    ],
    CpuUpdateMs => "cpu_update_ms" : Int { field cpu_update_ms } [
        "CPU update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    MemUpdateMs => "mem_update_ms" : Int { field mem_update_ms } [
        "Memory update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    DiskUpdateMs => "disk_update_ms" : Int { field disk_update_ms } [
        "Disk update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    NetUpdateMs => "net_update_ms" : Int { field net_update_ms } [
        "Network update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    GpuUpdateMs => "gpu_update_ms" : Int { field gpu_update_ms } [
        "GPU update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    ProcUpdateMs => "proc_update_ms" : Int { field proc_update_ms } [
        "Process update interval (ms).",
        "",
        "0 = use global update_ms.",
        "Range: 100 to 86400000.",
    ],
    NetDownload => "net_download" : Int { field net_download } [
        "Fixed download graph scale.",
        "",
        "Value in Mebibits. Default: 100.",
        "Overridden when auto scaling is on.",
    ],
    NetUpload => "net_upload" : Int { field net_upload } [
        "Fixed upload graph scale.",
        "",
        "Value in Mebibits. Default: 100.",
        "Overridden when auto scaling is on.",
    ],

    // -- string, direct field (free-form or constrained via choice_values) --
    ColorTheme => "color_theme" : String { field color_theme } [
        "Color theme.",
        "",
        "Choose from all bundled themes.",
        "",
        "\"default\" for the built-in theme.",
    ],
    ClockFormat => "clock_format" : String { field clock_format } [
        "Clock display format.",
        "",
        "Shown in the CPU widget. Uses format",
        "specifiers: %H, %M, %S, %X.",
        "",
        "Empty string to disable.",
    ],
    CustomCpuName => "custom_cpu_name" : String { field custom_cpu_name } [
        "Custom CPU name.",
        "",
        "Override the detected CPU model",
        "name. Empty string to disable.",
    ],
    ProcFilter => "proc_filter" : String { field proc_filter } [
        "Process filter.",
        "",
        "Filter by name. Prefix with !",
        "for inverse match.",
    ],

    // -- string, joined Vec<String> --
    DisksFilter => "disks_filter" : String { joined_vec disks_filter } [
        "Disk filter.",
        "",
        "Filter which disks are shown.",
        "Use drive letters (e.g. \"C:\").",
        "Prefix with ! to exclude.",
        "Separate with whitespace.",
    ],

    // -- string, layout-virtual --
    Shape => "shape" : String { shape } [
        "Layout shape (DSL).",
        "",
        "Recursive composition of vstack(...)",
        "and hstack(N:..., ...) wrappers around",
        "widget names: cpu, mem, net, proc,",
        "disk, gpu0..gpu7. Example:",
        "vstack(cpu, hstack(40:mem, 60:proc))",
    ],

    // -- string, fixed-array element --
    CustomGpuName0 => "custom_gpu_name0" : String { array custom_gpu_names[0] } [
        "Custom GPU name for GPU 0.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName1 => "custom_gpu_name1" : String { array custom_gpu_names[1] } [
        "Custom GPU name for GPU 1.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName2 => "custom_gpu_name2" : String { array custom_gpu_names[2] } [
        "Custom GPU name for GPU 2.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName3 => "custom_gpu_name3" : String { array custom_gpu_names[3] } [
        "Custom GPU name for GPU 3.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName4 => "custom_gpu_name4" : String { array custom_gpu_names[4] } [
        "Custom GPU name for GPU 4.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName5 => "custom_gpu_name5" : String { array custom_gpu_names[5] } [
        "Custom GPU name for GPU 5.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName6 => "custom_gpu_name6" : String { array custom_gpu_names[6] } [
        "Custom GPU name for GPU 6.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],
    CustomGpuName7 => "custom_gpu_name7" : String { array custom_gpu_names[7] } [
        "Custom GPU name for GPU 7.",
        "",
        "Override the detected GPU name.",
        "Empty string to disable.",
    ],

    // -- enum (typed enum field, browsable via choice_values) --
    GraphSymbol => "graph_symbol" : Enum { field graph_symbol } [
        "Default graph symbol.",
        "",
        "\"braille\" or \"block\".",
        "Per-widget overrides use \"default\"",
        "to inherit this setting.",
    ],
    GraphSymbolCpu => "graph_symbol_cpu" : Enum { field graph_symbol_cpu } [
        "CPU graph symbol.",
        "",
        "\"default\", \"braille\", or \"block\".",
    ],
    GraphSymbolNet => "graph_symbol_net" : Enum { field graph_symbol_net } [
        "Network graph symbol.",
        "",
        "\"default\", \"braille\", or \"block\".",
    ],
    GraphSymbolDisk => "graph_symbol_disk" : Enum { field graph_symbol_disk } [
        "Disk graph symbol.",
        "",
        "\"default\", \"braille\", or \"block\".",
    ],
    ProcSorting => "proc_sorting" : Enum { field proc_sorting } [
        "Process sort column.",
        "",
        "\"pid\", \"name\", \"command\",",
        "\"threads\", \"user\", \"memory\",",
        "or \"cpu\".",
    ],
    CpuGraphUpper => "cpu_graph_upper" : Enum { field cpu_graph_upper } [
        "Upper CPU graph source.",
        "",
        "CPU stat shown in the upper half",
        "of the CPU graph.",
    ],
    CpuGraphLower => "cpu_graph_lower" : Enum { field cpu_graph_lower } [
        "Lower CPU graph source.",
        "",
        "CPU stat shown in the lower half",
        "of the CPU graph.",
    ],
    TempScale => "temp_scale" : Enum { field temp_scale } [
        "Temperature scale.",
        "",
        "Celsius, Fahrenheit, Kelvin,",
        "or Rankine.",
    ],
    LogLevel => "log_level" : Enum { field log_level } [
        "Logging level.",
        "",
        "Sets verbosity for rtop.log.",
        "",
        "\"off\", \"error\", \"warn\",",
        "\"info\", \"debug\", or \"trace\".",
        "",
        "\"off\" disables file logging.",
        "Changes apply immediately.",
    ],
}

// ---------------------------------------------------------------------------
// ConfigKey — hand-written impls
// ---------------------------------------------------------------------------
//
// Methods below carry per-key validation logic, error handling, or
// shape-specific parsing that does not fit the declarative table
// above. Each one is intentionally hand-written.

impl ConfigKey {
    /// Set a value from its canonical string form.
    ///
    /// Returns `Err(SetStringError)` if `value` does not parse for
    /// the field's type. Used both by `cycle_browsable` (which
    /// passes a value drawn from `browsable_values()`, so failure
    /// is a contract violation) and by the inline-edit commit path
    /// (which validates first via [`Self::validate_string`] so the
    /// contract is also already satisfied).
    pub fn set_string(self, config: &mut Config, value: &str) -> Result<(), SetStringError> {
        let err = || SetStringError {
            key: self.name(),
            value: value.to_string(),
        };
        match self {
            Self::ColorTheme => config.color_theme = value.to_string(),
            Self::Shape => {
                config.set_shape(value).map_err(|_| err())?;
            }
            Self::GraphSymbol => config.graph_symbol = value.parse().map_err(|_| err())?,
            Self::GraphSymbolCpu => config.graph_symbol_cpu = value.parse().map_err(|_| err())?,
            Self::GraphSymbolNet => config.graph_symbol_net = value.parse().map_err(|_| err())?,
            Self::GraphSymbolDisk => config.graph_symbol_disk = value.parse().map_err(|_| err())?,
            Self::ProcSorting => config.proc_sorting = value.parse().map_err(|_| err())?,
            Self::CpuGraphUpper => config.cpu_graph_upper = value.parse().map_err(|_| err())?,
            Self::CpuGraphLower => config.cpu_graph_lower = value.parse().map_err(|_| err())?,

            Self::TempScale => config.temp_scale = value.parse().map_err(|_| err())?,
            Self::ClockFormat => config.clock_format = value.to_string(),
            Self::CustomCpuName => config.custom_cpu_name = value.to_string(),
            Self::DisksFilter => {
                config.disks_filter = Self::parse_disks_filter(value).map_err(|_| err())?;
            }
            Self::LogLevel => config.log_level = value.parse().map_err(|_| err())?,
            Self::ProcFilter => config.proc_filter = value.to_string(),
            Self::CustomGpuName0 => config.custom_gpu_names[0] = value.to_string(),
            Self::CustomGpuName1 => config.custom_gpu_names[1] = value.to_string(),
            Self::CustomGpuName2 => config.custom_gpu_names[2] = value.to_string(),
            Self::CustomGpuName3 => config.custom_gpu_names[3] = value.to_string(),
            Self::CustomGpuName4 => config.custom_gpu_names[4] = value.to_string(),
            Self::CustomGpuName5 => config.custom_gpu_names[5] = value.to_string(),
            Self::CustomGpuName6 => config.custom_gpu_names[6] = value.to_string(),
            Self::CustomGpuName7 => config.custom_gpu_names[7] = value.to_string(),
            // Bool and Int keys go through `set_bool` / `set_int`.
            // If the contract is violated we return an error
            // identifying the offending key.
            _ => return Err(err()),
        }
        Ok(())
    }

    /// Parse `value` as a whitespace-separated list of drive filter
    /// entries (e.g. `"C: !D:"`).
    ///
    /// An empty list is allowed (matches every disk). Each entry
    /// must be a single ASCII letter followed by `:`, optionally
    /// prefixed with `!`. Returns the original token list (so case
    /// and `!` prefixes are preserved verbatim for round-trip
    /// fidelity) — normalisation happens at match time.
    pub fn parse_disks_filter(value: &str) -> Result<Vec<String>, &'static str> {
        let tokens: Vec<String> = value.split_whitespace().map(str::to_string).collect();
        let parsed = crate::domain::disk::DisksFilter::parse(&tokens);
        if !parsed.invalid().is_empty() {
            return Err("drive entries must be like 'C:' or '!D:'");
        }
        Ok(tokens)
    }

    /// Parse `value` as an integer for this int-typed key, enforcing
    /// the same bounds that [`Config::validate`] would clamp to.
    ///
    /// Calling this on a non-int key returns an error rather than
    /// panicking so the inline editor can degrade gracefully.
    pub fn parse_int(self, value: &str) -> Result<i64, &'static str> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("must be an integer");
        }
        let n: i64 = trimmed.parse().map_err(|_| "must be an integer")?;
        let in_range = match self {
            Self::UpdateMs => (100..=86_400_000).contains(&n),
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => (0..=86_400_000).contains(&n),
            Self::NetDownload | Self::NetUpload => (0..=10_000_000).contains(&n),
            _ => return Err("not an integer key"),
        };
        if in_range {
            Ok(n)
        } else {
            Err(self.int_bounds_message())
        }
    }

    fn int_bounds_message(self) -> &'static str {
        match self {
            Self::UpdateMs => "must be 100..86400000 ms",
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => "must be 0..86400000 ms (0=inherit)",
            Self::NetDownload | Self::NetUpload => "must be 0..10000000 KiB/s",
            _ => "out of range",
        }
    }

    /// Validate that `value` is acceptable for this string-typed key
    /// without mutating any config field. Returns a static error
    /// message suitable for inline display in the options menu when
    /// validation fails.
    ///
    /// Used by the inline editor commit path: validate first, and
    /// only call [`Self::set_string`] when validation succeeds.
    pub fn validate_string(self, value: &str) -> Result<(), &'static str> {
        match self {
            Self::Shape => value
                .parse::<Slot>()
                .map(|_| ())
                .map_err(|_| "invalid layout shape"),
            Self::DisksFilter => Self::parse_disks_filter(value).map(|_| ()),
            Self::ClockFormat
            | Self::CustomCpuName
            | Self::ProcFilter
            | Self::CustomGpuName0
            | Self::CustomGpuName1
            | Self::CustomGpuName2
            | Self::CustomGpuName3
            | Self::CustomGpuName4
            | Self::CustomGpuName5
            | Self::CustomGpuName6
            | Self::CustomGpuName7 => Ok(()),
            _ => match self.kind() {
                KeyKind::String | KeyKind::Enum => {
                    if self.browsable_values().contains(&value) {
                        Ok(())
                    } else {
                        Err("invalid value")
                    }
                }
                _ => Err("not a string key"),
            },
        }
    }

    /// Returns the allowed values for constrained string keys, or None for free-form.
    pub fn choice_values(self) -> Option<&'static [&'static str]> {
        match self {
            Self::ColorTheme => Some(crate::theme::THEME_NAMES),
            Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolDisk => Some(GraphSymbol::NAMES),
            Self::CpuGraphUpper | Self::CpuGraphLower => Some(CpuGraphSource::NAMES),
            Self::TempScale => Some(TempScale::NAMES),
            Self::ProcSorting => Some(ProcSort::NAMES),
            Self::LogLevel => Some(crate::log::FILTER_NAMES),
            _ => None,
        }
    }

    /// Returns the canonical values cycled by the options menu for
    /// browsable keys, or `&[]` for free-form keys.
    pub fn browsable_values(self) -> &'static [&'static str] {
        self.choice_values().unwrap_or(&[])
    }
}

fn bool_display(v: bool) -> String {
    if v {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_file_uses_defaults() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_empty.toml");
        fs::write(&tmp, "").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.update_ms, 2000);
        assert_eq!(config.color_theme, "default");
        assert!(config.theme_background);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_valid_config_parses_all_types() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_valid.toml");
        fs::write(
            &tmp,
            "color_theme = \"dracula\"\ntheme_background = false\nupdate_ms = 500\n",
        )
        .unwrap();

        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.color_theme, "dracula");
        assert!(!config.theme_background);
        assert_eq!(config.update_ms, 500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_preserves_comments() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_comments.toml");
        fs::write(&tmp, "# this is a comment\nupdate_ms = 1000\n").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.update_ms, 1000);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_unknown_keys_are_ignored() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_unknown.toml");
        fs::write(&tmp, "nonexistent_key = \"value\"\n").unwrap();
        let warnings = config.load(&tmp);
        // serde with #[serde(default)] silently ignores unknown keys
        assert!(warnings.is_empty());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_toml_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_badtoml.toml");
        fs::write(&tmp, "this is not valid toml {{{\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("parse"));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_out_of_range_int_generates_warning() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_out_of_range.toml");
        fs::write(&tmp, "update_ms = 50\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("out of range"));
        assert_eq!(config.update_ms, 100);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_typed_value_aborts_whole_parse() {
        // An invalid value for a typed enum field causes serde to
        // fail the whole `toml::from_str::<Config>` and the loader
        // returns a single "Failed to parse config" warning; the
        // entire config falls back to defaults.
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_typed_value.toml");
        fs::write(&tmp, "graph_symbol = \"ascii\"\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Failed to parse config"));
        assert!(warnings[0].contains("graph_symbol"));
        // Defaults preserved.
        assert_eq!(config.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.color_theme, "default");
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_string_choice_resets_just_that_field() {
        // String-typed fields validated by `validate_choice` reset
        // the offending field and emit a per-field warning while
        // the rest of the config loads normally.
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_bad_string_values.toml");
        fs::write(&tmp, "color_theme = \"foo\"\n").unwrap();
        let warnings = config.load(&tmp);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("color_theme") && w.contains("foo"))
        );
        assert_eq!(config.color_theme, "default");
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn write_roundtrip_preserves_values() {
        let mut config = Config::new();
        config.color_theme = "nord".to_string();
        config.vim_keys = true;
        config.update_ms = 1500;

        let tmp = std::env::temp_dir().join("rtop_test_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut config2 = Config::new();
        config2.load(&tmp);
        assert_eq!(config2.color_theme, "nord");
        assert!(config2.vim_keys);
        assert_eq!(config2.update_ms, 1500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn defaults_are_correct() {
        let config = Config::new();
        assert!(config.theme_background);
        assert!(!config.vim_keys);
        assert_eq!(config.update_ms, 2000);
        assert_eq!(config.color_theme, "default");
        assert_eq!(config.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.proc_sorting, ProcSort::Cpu);
        // First-launch cursor lands on the custom preset so the
        // visible layout matches what's persisted in TOML. Custom's
        // default tree is the `all` preset's layout — the dashboard
        // view straight away.
        assert_eq!(
            config.preset.active(),
            crate::domain::preset::ActivePreset::Custom
        );
        assert_eq!(
            config.layout_spec(),
            crate::domain::preset::BuiltinPreset::All.layout_spec(),
        );
    }

    #[test]
    fn validate_clamps_ints() {
        let mut config = Config::new();
        config.update_ms = 50;
        config.net_download = -1;
        let warnings = config.validate();
        assert_eq!(config.update_ms, 100);
        assert_eq!(config.net_download, 0);
        assert!(warnings.len() >= 2);
    }

    #[test]
    fn validate_keeps_valid_disks_filter_unchanged() {
        let mut config = Config::new();
        config.disks_filter = vec!["C:".into(), "!D:".into()];
        let warnings = config.validate();
        assert!(
            !warnings.iter().any(|w| w.contains("disks_filter")),
            "valid disks_filter should not warn, got: {warnings:?}"
        );
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn validate_warns_and_strips_invalid_disks_filter_entries() {
        let mut config = Config::new();
        config.disks_filter = vec![
            "C:".into(),
            "abc".into(),
            "D:".into(),
            "3".into(),
            "!!".into(),
        ];
        let warnings = config.validate();
        let disks_warning = warnings
            .iter()
            .find(|w| w.contains("disks_filter"))
            .expect("validate must surface a disks_filter warning");
        assert!(disks_warning.contains("abc"));
        assert!(disks_warning.contains('3'));
        assert!(disks_warning.contains("!!"));
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "D:".to_string()]
        );
    }

    #[test]
    fn validate_empty_disks_filter_does_not_warn() {
        let mut config = Config::new();
        config.disks_filter = Vec::new();
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("disks_filter")));
        assert!(config.disks_filter.is_empty());
    }

    #[test]
    fn toggle_widget_adds_when_missing() {
        let mut config = Config::new();
        // Start from a controlled custom layout (Cpu + Mem only).
        config.custom.root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        config.toggle_widget(WidgetKind::Net);
        assert!(config.layout_spec().contains(WidgetKind::Net));
        assert!(config.custom.root.contains(WidgetKind::Net));
    }

    #[test]
    fn toggle_widget_removes_when_present() {
        let mut config = Config::new();
        config.custom.root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Net),
        ]);
        config.toggle_widget(WidgetKind::Net);
        assert!(!config.layout_spec().contains(WidgetKind::Net));
        assert!(!config.custom.root.contains(WidgetKind::Net));
    }

    #[test]
    fn toggle_widget_does_not_empty_the_tree() {
        // Toggling off the only visible widget is a no-op — every
        // custom layout must contain at least one widget.
        let mut config = Config::new();
        config.custom.root = Slot::Widget(WidgetKind::Cpu);
        config.toggle_widget(WidgetKind::Cpu);
        assert!(
            config.custom.root.contains(WidgetKind::Cpu),
            "single-widget tree must not be emptiable",
        );
    }

    #[test]
    fn toggle_widget_on_builtin_promotes_to_custom() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        // Move to a builtin ("all") explicitly.
        config.preset.set(ActivePreset::Builtin(BuiltinPreset::All));
        // Pre-condition: live layout includes CPU.
        assert!(config.layout_spec().contains(WidgetKind::Cpu));

        config.toggle_widget(WidgetKind::Cpu);

        // Cursor switched to custom and custom dropped CPU.
        assert_eq!(config.preset.active(), ActivePreset::Custom);
        assert!(!config.custom.root.contains(WidgetKind::Cpu));
        assert!(!config.layout_spec().contains(WidgetKind::Cpu));
    }

    #[test]
    fn toggle_widget_on_custom_does_not_change_cursor() {
        let mut config = Config::new();
        // Default cursor is custom.
        let before = config.preset.active();
        config.toggle_widget(WidgetKind::Cpu);
        assert_eq!(config.preset.active(), before);
    }

    #[test]
    fn preset_default_is_custom() {
        let config = Config::new();
        assert_eq!(
            config.preset.active(),
            crate::domain::preset::ActivePreset::Custom
        );
    }

    #[test]
    fn cycle_preset_forward_walks_full_cycle_then_wraps() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config.preset.set(ActivePreset::Builtin(BuiltinPreset::All));
        for _ in 0..ActivePreset::CYCLE_LEN {
            config.cycle_preset(true);
        }
        // After one full lap forward we are back where we started.
        assert_eq!(
            config.preset.active(),
            ActivePreset::Builtin(BuiltinPreset::All)
        );
        config.cycle_preset(false);
        // One step backward from "all" lands on Custom (the slot
        // immediately before the first builtin in cycle order).
        assert_eq!(config.preset.active(), ActivePreset::Custom);
    }

    #[test]
    fn cycle_preset_to_builtin_dispatches_to_builtin_layout() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config.custom.root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        ]);
        config.preset.set(ActivePreset::Custom);
        // On custom, live = custom.
        assert_eq!(config.layout_spec(), config.custom.root);

        // Cycle to builtin "all". Live now reads from the builtin.
        config.preset.set(ActivePreset::Builtin(BuiltinPreset::All));
        assert_eq!(config.layout_spec(), BuiltinPreset::All.layout_spec());
        assert!(config.layout_spec().contains(WidgetKind::Cpu));
        assert!(config.layout_spec().contains(WidgetKind::Disk));
        // Custom storage is untouched by cycling.
        assert_eq!(
            config.custom.root,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
            ])
        );
    }

    #[test]
    fn cycle_preset_back_to_custom_restores_user_layout() {
        use crate::domain::preset::ActivePreset;
        let mut config = Config::new();
        config.custom.root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
            Slot::Widget(WidgetKind::Gpu(0)),
        ]);
        config.preset.set(ActivePreset::Custom);

        // Visit a builtin and come back.
        config.cycle_preset(true);
        config.cycle_preset(false);
        assert_eq!(config.preset.active(), ActivePreset::Custom);
        assert_eq!(
            config.layout_spec(),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Gpu(0)),
            ])
        );
    }
    #[test]
    fn unknown_preset_name_warns_and_falls_back_to_custom() {
        let path = std::env::temp_dir().join("rtop_test_bad_preset_name.toml");
        std::fs::write(&path, "preset = \"cpu+pro\"\n").unwrap();

        let mut config = Config::new();
        let warnings = config.load(&path);

        assert_eq!(
            config.preset.active(),
            crate::domain::preset::ActivePreset::Custom
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("preset") && w.contains("cpu+pro")),
            "expected a warning naming the offending preset value, got {warnings:?}"
        );
    }

    #[test]
    fn config_round_trips_custom_layout_through_toml() {
        let mut config = Config::new();
        config.custom.root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        let tmp = std::env::temp_dir().join("rtop_test_layout_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut loaded = Config::new();
        loaded.load(&tmp);
        // Cursor on custom (default), so live should match custom.
        assert_eq!(
            loaded.layout_spec(),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn config_key_parse_returns_none_for_unknown() {
        assert_eq!(ConfigKey::parse("nonexistent"), None);
    }

    #[test]
    fn config_key_roundtrip_through_parser() {
        let keys = [
            ConfigKey::ColorTheme,
            ConfigKey::ProcSorting,
            ConfigKey::ThemeBackground,
            ConfigKey::UpdateMs,
            ConfigKey::NetDownload,
        ];

        for key in keys {
            assert_eq!(ConfigKey::parse(key.name()), Some(key));
        }
    }

    // -----------------------------------------------------------------
    // Inline editor parser/validator/set_widgets coverage
    // -----------------------------------------------------------------

    #[test]
    fn parse_disks_filter_accepts_empty() {
        assert_eq!(
            ConfigKey::parse_disks_filter("").unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            ConfigKey::parse_disks_filter("   ").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parse_disks_filter_preserves_case_and_prefix() {
        let result = ConfigKey::parse_disks_filter("c: !D: E:").unwrap();
        assert_eq!(
            result,
            vec!["c:".to_string(), "!D:".to_string(), "E:".to_string()]
        );
    }

    #[test]
    fn parse_disks_filter_rejects_bare_letter() {
        assert!(ConfigKey::parse_disks_filter("C").is_err());
        assert!(ConfigKey::parse_disks_filter("C: D").is_err());
        assert!(ConfigKey::parse_disks_filter("!E").is_err());
    }

    #[test]
    fn parse_int_enforces_update_ms_lower_bound() {
        assert!(ConfigKey::UpdateMs.parse_int("99").is_err());
        assert_eq!(ConfigKey::UpdateMs.parse_int("100").unwrap(), 100);
    }

    #[test]
    fn parse_int_accepts_zero_for_inherit_keys() {
        for key in [
            ConfigKey::CpuUpdateMs,
            ConfigKey::MemUpdateMs,
            ConfigKey::DiskUpdateMs,
            ConfigKey::NetUpdateMs,
            ConfigKey::GpuUpdateMs,
            ConfigKey::ProcUpdateMs,
        ] {
            assert_eq!(key.parse_int("0").unwrap(), 0);
        }
    }

    #[test]
    fn parse_int_rejects_negative_and_overflow() {
        assert!(ConfigKey::CpuUpdateMs.parse_int("-1").is_err());
        assert!(ConfigKey::NetDownload.parse_int("10000001").is_err());
        assert_eq!(
            ConfigKey::NetDownload.parse_int("10000000").unwrap(),
            10_000_000
        );
    }

    #[test]
    fn parse_int_trims_whitespace() {
        assert_eq!(ConfigKey::UpdateMs.parse_int("  500  ").unwrap(), 500);
    }

    #[test]
    fn parse_int_rejects_non_numeric_or_empty() {
        assert!(ConfigKey::UpdateMs.parse_int("abc").is_err());
        assert!(ConfigKey::UpdateMs.parse_int("").is_err());
    }

    #[test]
    fn parse_int_rejects_non_int_key() {
        assert!(ConfigKey::ColorTheme.parse_int("100").is_err());
        assert!(ConfigKey::Shape.parse_int("100").is_err());
    }

    #[test]
    fn validate_string_shape() {
        assert!(ConfigKey::Shape.validate_string("vstack(cpu, mem)").is_ok());
        assert!(ConfigKey::Shape.validate_string("cpu").is_ok());
        assert!(ConfigKey::Shape.validate_string("").is_err());
        assert!(ConfigKey::Shape.validate_string("nope").is_err());
        assert!(
            ConfigKey::Shape
                .validate_string("vstack(cpu, cpu)")
                .is_err()
        );
    }

    #[test]
    fn validate_string_disks_filter() {
        assert!(ConfigKey::DisksFilter.validate_string("").is_ok());
        assert!(ConfigKey::DisksFilter.validate_string("C: !D:").is_ok());
        assert!(ConfigKey::DisksFilter.validate_string("X").is_err());
    }

    #[test]
    fn validate_string_free_form_keys_always_ok() {
        for key in [
            ConfigKey::ClockFormat,
            ConfigKey::CustomCpuName,
            ConfigKey::ProcFilter,
            ConfigKey::CustomGpuName0,
            ConfigKey::CustomGpuName7,
        ] {
            assert!(key.validate_string("").is_ok());
            assert!(key.validate_string("anything goes !@#$%").is_ok());
        }
    }

    #[test]
    fn validate_string_constrained_choice_keys() {
        assert!(ConfigKey::ColorTheme.validate_string("default").is_ok());
        assert!(
            ConfigKey::ColorTheme
                .validate_string("nonexistent")
                .is_err()
        );
        assert!(ConfigKey::LogLevel.validate_string("info").is_ok());
        assert!(ConfigKey::LogLevel.validate_string("loud").is_err());
    }

    #[test]
    fn validate_string_rejects_non_string_keys() {
        assert!(ConfigKey::UpdateMs.validate_string("100").is_err());
        assert!(ConfigKey::RoundedCorners.validate_string("true").is_err());
    }

    #[test]
    fn set_shape_no_op_keeps_builtin_cursor() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        let cursor = ActivePreset::Builtin(BuiltinPreset::All);
        config.preset.set(cursor);
        // Setting shape to the live value must not promote.
        let live = config.layout_spec().to_string();
        config.set_shape(&live).unwrap();
        assert_eq!(
            config.preset.active(),
            cursor,
            "no-op must not promote to custom"
        );
    }

    #[test]
    fn set_shape_change_promotes_builtin_to_custom() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config
            .preset
            .set(ActivePreset::Builtin(BuiltinPreset::CpuProc));
        config.set_shape("vstack(cpu, mem)").unwrap();
        assert_eq!(config.preset.active(), ActivePreset::Custom);
        assert_eq!(
            config.custom.root,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
    }

    #[test]
    fn set_shape_on_custom_writes_without_changing_cursor() {
        use crate::domain::preset::ActivePreset;
        let mut config = Config::new();
        config.preset.set(ActivePreset::Custom);
        config.set_shape("cpu").unwrap();
        assert_eq!(config.preset.active(), ActivePreset::Custom);
        assert_eq!(config.custom.root, Slot::Widget(WidgetKind::Cpu));
    }

    #[test]
    fn set_string_shape_via_inline_editor_path() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config
            .preset
            .set(ActivePreset::Builtin(BuiltinPreset::CpuProc));
        ConfigKey::Shape
            .set_string(&mut config, "vstack(cpu, mem, disk)")
            .unwrap();
        assert_eq!(config.preset.active(), ActivePreset::Custom);
        assert_eq!(
            config.custom.root,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Disk),
            ])
        );
    }

    #[test]
    fn set_string_shape_invalid_returns_err() {
        let mut config = Config::new();
        assert!(ConfigKey::Shape.set_string(&mut config, "nope").is_err());
        assert!(ConfigKey::Shape.set_string(&mut config, "").is_err());
        // Duplicate widget kinds rejected at parse time.
        assert!(
            ConfigKey::Shape
                .set_string(&mut config, "vstack(cpu, cpu)")
                .is_err()
        );
    }

    #[test]
    fn set_string_disks_filter_via_inline_editor_path() {
        let mut config = Config::new();
        ConfigKey::DisksFilter
            .set_string(&mut config, "C: !D:")
            .unwrap();
        assert_eq!(
            config.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn set_string_disks_filter_invalid_returns_err() {
        let mut config = Config::new();
        assert!(ConfigKey::DisksFilter.set_string(&mut config, "X").is_err());
    }
}
