use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::{CpuGraphSource, GraphSymbol, TempScale};
use crate::domain::layout_spec::Slot;
use crate::domain::preset::default_custom_layout;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing_subscriber::filter::LevelFilter;

/// Maximum number of GPUs supported.
pub const MAX_GPUS: usize = 8;

/// Error returned by [`StringKey::set`] / [`EnumKey::set_canonical`]
/// when the supplied string cannot be parsed into the field's value
/// type.
///
/// Constructed at the call sites where parse failure is a contract
/// violation (the inline-edit commit path validates first) or where
/// the caller wants to surface the offending key + value verbatim
/// in a log message.
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

// ---------------------------------------------------------------------------
// Per-subsystem config sections
// ---------------------------------------------------------------------------
//
// `Config` was originally a 50+-field flat struct; renderers and
// handlers reached into it for a few unrelated fields each, and
// adding a per-widget setting touched the same struct as adding a
// log-level setting.
//
// The new shape splits the persisted state into per-subsystem
// sub-structs (`UiConfig`, `RefreshConfig`, `CpuConfig`, …) that
// `Config` composes via `#[serde(flatten)]`. The on-disk TOML
// format is unchanged — every field still serialises at the
// top level — but the in-memory layout matches the natural
// dependency boundaries:
//
// * Widget renderers can take `&CpuConfig` (or `&NetConfig`, etc.)
//   directly, eliminating the per-widget `*WidgetSettings`
//   adapter layer in a follow-up todo.
// * Config consumers that only care about refresh intervals don't
//   pull in CPU/disk/network field knowledge.
// * Adding a new option is a one-section edit, not a 50-field
//   struct edit.
//
// Top-level fields (`preset`, `custom_layout`, `hidden_widgets`,
// `log_level`, `conf_file`) stay on `Config` directly because
// they don't fit a single subsystem (layout selection,
// process-wide log filter, runtime view filter, file path).

/// UI / general appearance settings (`general` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme_background: bool,
    pub rounded_corners: bool,
    pub color_theme: String,
    pub vim_keys: bool,
    pub terminal_sync: bool,
    pub background_update: bool,
    pub base_10_sizes: bool,
    /// Default graph drawing style; per-widget `graph_symbol_*`
    /// fields override this when set to a non-`Default` value.
    pub graph_symbol: GraphSymbol,
    /// Format string for the clock displayed in the CPU widget
    /// header. Empty hides the clock.
    pub clock_format: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme_background: true,
            rounded_corners: true,
            color_theme: "default".to_string(),
            vim_keys: false,
            terminal_sync: true,
            background_update: true,
            base_10_sizes: false,
            graph_symbol: GraphSymbol::Braille,
            clock_format: "%X".to_string(),
        }
    }
}

/// Refresh-interval settings (one global + per-subsystem
/// overrides). 0 on a per-subsystem field means "inherit the
/// global `update_ms`".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshConfig {
    pub update_ms: i64,
    pub cpu_update_ms: i64,
    pub mem_update_ms: i64,
    pub disk_update_ms: i64,
    pub net_update_ms: i64,
    pub gpu_update_ms: i64,
    pub proc_update_ms: i64,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            update_ms: 2000,
            cpu_update_ms: 0,
            mem_update_ms: 0,
            disk_update_ms: 0,
            net_update_ms: 0,
            gpu_update_ms: 0,
            proc_update_ms: 0,
        }
    }
}

/// CPU widget settings (`cpu` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuConfig {
    pub graph_symbol_cpu: GraphSymbol,
    pub cpu_graph_upper: CpuGraphSource,
    pub cpu_graph_lower: CpuGraphSource,
    pub cpu_invert_lower: bool,
    pub cpu_single_graph: bool,
    pub cpu_auto_scale: bool,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub temp_scale: TempScale,
    pub show_cpu_freq: bool,
    pub custom_cpu_name: String,
    pub show_uptime: bool,
    pub show_cpu_watts: bool,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            graph_symbol_cpu: GraphSymbol::Default,
            cpu_graph_upper: CpuGraphSource::User,
            cpu_graph_lower: CpuGraphSource::System,
            cpu_invert_lower: true,
            cpu_single_graph: false,
            cpu_auto_scale: false,
            check_temp: true,
            show_coretemp: true,
            temp_scale: TempScale::Celsius,
            show_cpu_freq: true,
            custom_cpu_name: String::new(),
            show_uptime: true,
            show_cpu_watts: true,
        }
    }
}

/// Memory widget settings (`mem` options-menu tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemConfig {
    pub show_swap: bool,
}

impl Default for MemConfig {
    fn default() -> Self {
        Self { show_swap: true }
    }
}

/// Network widget settings (`net` options-menu tab).
///
/// Runtime-mutable network bits (`net_auto`, `net_sync`,
/// `net_iface`) live on [`ViewConfig`] / `AppState::view`
/// ([`crate::app::RuntimeView`]) — they're toggled outside the
/// options menu (`a`, `s`, Tab keys) and the architecture treats
/// them as session view-state that mirrors back to the persisted
/// form on save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetConfig {
    pub graph_symbol_net: GraphSymbol,
    pub swap_upload_download: bool,
    pub net_download: i64,
    pub net_upload: i64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            graph_symbol_net: GraphSymbol::Default,
            swap_upload_download: false,
            net_download: 100,
            net_upload: 100,
        }
    }
}

/// Process widget settings (`proc` options-menu tab).
///
/// Runtime-mutable process bits (`proc_tree`, `proc_reversed`,
/// `proc_per_core`, `proc_sorting`, `proc_filter`) live on
/// [`ViewConfig`] / `AppState::view` ([`crate::app::RuntimeView`])
/// — they're toggled outside the options menu (`e`, `r`, `c`,
/// Left/Right, `f`/`/` keys) and the architecture treats them as
/// session view-state that mirrors back to the persisted form on
/// save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcConfig {
    pub proc_aggregate: bool,
    pub proc_colors: bool,
    pub proc_gradient: bool,
    pub proc_mem_bytes: bool,
    pub keep_dead_proc_usage: bool,
}

impl Default for ProcConfig {
    fn default() -> Self {
        Self {
            proc_aggregate: false,
            proc_colors: true,
            proc_gradient: true,
            proc_mem_bytes: true,
            keep_dead_proc_usage: false,
        }
    }
}

/// GPU widget settings (`gpu` options-menu tab).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuConfig {
    pub custom_gpu_names: [String; MAX_GPUS],
}

/// Disk widget settings (`disk` options-menu tab).
///
/// Runtime-mutable disk bits (`io_mode`) live on [`ViewConfig`] /
/// `AppState::view` ([`crate::app::RuntimeView`]) — `i` toggles it
/// outside the options menu and the architecture treats it as
/// session view-state that mirrors back to the persisted form on
/// save.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskConfig {
    pub graph_symbol_disk: GraphSymbol,
    pub show_io_stat: bool,
    pub io_graph_combined: bool,
    pub disk_io_mode: bool,
    pub disks_filter: Vec<String>,
}

impl Default for DiskConfig {
    fn default() -> Self {
        Self {
            graph_symbol_disk: GraphSymbol::Default,
            show_io_stat: true,
            io_graph_combined: false,
            disk_io_mode: false,
            disks_filter: Vec::new(),
        }
    }
}

/// View-state config: fields the user toggles outside the
/// options menu (process tree mode, sort, filter, IO mode,
/// network auto-scale / sync / interface).
///
/// These fields persist across restarts (so toggle gestures
/// survive the next launch) but their *runtime* mutation goes
/// through [`crate::app::RuntimeView`] in `AppState`, not
/// directly through `Config`. Sync points:
///
/// 1. `AppState::new` initialises `RuntimeView` from
///    `Config::view`.
/// 2. Opening the options menu copies `RuntimeView ->
///    Config::view` so the menu shows the current values.
/// 3. Committing an options-menu edit copies
///    `Config::view -> RuntimeView` so the runtime picks up the
///    user's change.
/// 4. Process exit copies `RuntimeView -> Config::view`
///    before serialising so the on-disk form is current.
///
/// Handler runtime toggles (`e`, `r`, `c`, Left/Right, `i`, `a`,
/// `s`, Tab, `f`/`/`) mutate `RuntimeView` only — they never
/// reach `&mut Config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    pub proc_tree: bool,
    pub proc_reversed: bool,
    pub proc_per_core: bool,
    pub proc_sorting: ProcSort,
    pub proc_filter: String,
    pub io_mode: bool,
    pub net_auto: bool,
    pub net_sync: bool,
    /// Preferred network interface name. `"auto"` picks the first
    /// available interface; any other value is the user's pinned
    /// selection (cycled at runtime via `Tab` / `Shift+Tab`).
    pub net_iface: String,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            proc_tree: false,
            proc_reversed: false,
            proc_per_core: false,
            proc_sorting: ProcSort::Cpu,
            proc_filter: String::new(),
            io_mode: false,
            net_auto: true,
            net_sync: false,
            net_iface: "auto".to_string(),
        }
    }
}

/// Logging config — process-wide tracing level. Lives in its own
/// section purely so the `config_schema!` macro can address it
/// with the same `section.field` shape it uses for every other
/// key; logical owner is the logging subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// Process-wide tracing log level. Serialised as the canonical
    /// lowercase name (`off`/`error`/`warn`/`info`/`debug`/`trace`).
    #[serde(
        with = "crate::log::serde_filter",
        default = "crate::log::default_filter"
    )]
    pub log_level: LevelFilter,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            log_level: LevelFilter::WARN,
        }
    }
}

// ---------------------------------------------------------------------------
// Config — composed wrapper over per-subsystem sections
// ---------------------------------------------------------------------------

/// All persisted configuration state for rtop.
///
/// Composed of per-subsystem sections (see the `*Config` structs
/// above) flattened into a single TOML root via
/// `#[serde(flatten)]`. Top-level fields (`preset`,
/// `custom_layout`, `hidden_widgets`, `log_level`, `conf_file`)
/// stay on the wrapper because they don't fit a single subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(flatten)]
    pub ui: UiConfig,
    #[serde(flatten)]
    pub refresh: RefreshConfig,
    #[serde(flatten)]
    pub cpu: CpuConfig,
    #[serde(flatten)]
    pub mem: MemConfig,
    #[serde(flatten)]
    pub net: NetConfig,
    #[serde(flatten)]
    pub proc: ProcConfig,
    #[serde(flatten)]
    pub gpu: GpuConfig,
    #[serde(flatten)]
    pub disk: DiskConfig,
    #[serde(flatten)]
    pub log: LogConfig,
    /// View-state (runtime-toggle) section. Mirrored at runtime
    /// by [`crate::app::RuntimeView`]; see [`ViewConfig`] for the
    /// sync-point contract.
    #[serde(flatten)]
    pub view: ViewConfig,

    /// Cursor over the active preset (one of the builtins, or the
    /// custom slot). Persisted by canonical name. Private so all
    /// writes go through [`Self::cycle_preset`] /
    /// [`Self::set_active_preset`], which keep the layout cache in
    /// sync. Read access is via [`Self::active_preset`].
    preset: crate::domain::preset::PresetField,

    /// Persisted layout for the `Custom` preset slot. Stored as a
    /// flat top-level `custom_layout` TOML key carrying the DSL
    /// string form of the [`Slot`] tree:
    ///
    /// ```toml
    /// custom_layout = "vstack(cpu, hstack(40:mem, 60:proc))"
    /// ```
    ///
    /// Mutated only by the user editing this field in the options
    /// menu or rewriting the TOML key directly. Toggle keys
    /// (`1`-`9`/`0`) operate on the runtime view filter and never
    /// touch this field.
    pub custom_layout: Slot,

    /// Persisted runtime view filter. The user toggles widgets in
    /// or out of this set with `1`-`9` / `0` / `Shift+R`; the set
    /// survives restart so a hidden-on-Monday widget stays hidden
    /// on Tuesday. The cursor and active layout are unaffected by
    /// filter membership — see `AppState::filter` for the live
    /// runtime mirror and the engine-side composition.
    ///
    /// Serialised as a top-level `hidden_widgets` array of widget
    /// names. Empty array (the default) writes the field on save
    /// to make the persistence behaviour discoverable.
    #[serde(default)]
    pub hidden_widgets: crate::domain::widget_set::WidgetSet,

    /// Cached active layout. Materialised by [`Self::layout_spec`]
    /// from `preset` + `custom_layout`. Invalidated by every
    /// mutating method that touches those two fields
    /// ([`Self::cycle_preset`], [`Self::set_active_preset`],
    /// [`Self::set_custom_layout`], [`Self::apply_defaults`],
    /// [`Self::load`]). Per-frame `layout_spec()` borrows from this
    /// cache so the engine never re-clones the `Slot` tree.
    #[serde(skip)]
    layout_cache: std::cell::OnceCell<Slot>,

    /// Path to the loaded config file (for `reload()`). Not
    /// persisted — populated by `load()` at startup.
    #[serde(skip)]
    conf_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui: UiConfig::default(),
            refresh: RefreshConfig::default(),
            cpu: CpuConfig::default(),
            mem: MemConfig::default(),
            net: NetConfig::default(),
            proc: ProcConfig::default(),
            gpu: GpuConfig::default(),
            disk: DiskConfig::default(),
            log: LogConfig::default(),
            view: ViewConfig::default(),

            // First-launch cursor lands on the `all` builtin so the
            // very first `p` keypress visibly cycles to a different
            // preset, and the cursor name matches the visible
            // layout. Custom is reached by cycling all the way
            // around or by editing the layout (toggle keys, options
            // menu, or `custom_layout` DSL string in `rtop.toml`),
            // at which point the cursor auto-promotes.
            preset: crate::domain::preset::PresetField::default(),

            // First-launch custom layout: a clone of the `all`
            // preset's tree (see `default_custom_layout`). The
            // cursor is on `all` builtin by default so this slot is
            // dormant until the user edits something.
            custom_layout: default_custom_layout(),

            // First-launch view filter is empty — every widget the
            // active preset exposes is visible. The field is
            // persisted so toggle gestures (1-9, 0, Shift+R)
            // survive restart.
            hidden_widgets: crate::domain::widget_set::WidgetSet::new(),

            layout_cache: std::cell::OnceCell::new(),

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
            self.refresh.update_ms.max(100) as u64
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

        // Clamp every integer field to its declared bounds. Each
        // IntKey owns its `[min, max]` range in one place; this loop
        // is the only consumer that mutates out-of-range values
        // back into range.
        for &key in IntKey::ALL {
            let old = key.get(self);
            let new = key.clamp_to_bounds(old);
            if new != old {
                warnings.push(format!(
                    "Value for '{}' out of range ({old}), clamped to {new}",
                    key.name(),
                ));
                key.set(self, new);
            }
        }

        // The only remaining `validate_choice` call covers
        // `color_theme` — the single string-typed field with a
        // closed set of choices. The other browsable config
        // fields are typed enums whose validity is enforced by
        // serde at deserialise time.
        validate_choice(
            &mut self.ui.color_theme,
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
        let filter = crate::domain::disk::DisksFilter::parse(&self.disk.disks_filter);
        if !filter.invalid().is_empty() {
            warnings.push(format!(
                "Invalid drive entry/entries in 'disks_filter': {}",
                filter.invalid().join(", ")
            ));
            let invalid_tokens = filter.invalid().to_vec();
            self.disk
                .disks_filter
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
    /// custom preset returns a clone of its persisted tree. Cached
    /// after first call; mutators ([`Self::cycle_preset`],
    /// [`Self::set_active_preset`], [`Self::set_custom_layout`])
    /// invalidate via [`Self::invalidate_layout_cache`].
    pub fn layout_spec(&self) -> &Slot {
        self.layout_cache
            .get_or_init(|| match self.preset.active() {
                crate::domain::preset::ActivePreset::Builtin(b) => b.layout_spec(),
                crate::domain::preset::ActivePreset::Custom => self.custom_layout.clone(),
            })
    }

    /// Drop the cached active layout. Called automatically by every
    /// mutator that touches `preset` or `custom_layout` ([`Self::cycle_preset`],
    /// [`Self::set_active_preset`], [`Self::set_custom_layout`]),
    /// so callers normally don't need to invoke this directly.
    /// Public so future code paths that mutate cache inputs in
    /// non-method form (e.g. test helpers, custom deserialise hooks)
    /// can keep the cache honest.
    pub fn invalidate_layout_cache(&mut self) {
        self.layout_cache = std::cell::OnceCell::new();
    }

    /// Read the active preset cursor. Replaces the previous direct
    /// `config.active_preset()` access now that `preset` is private.
    pub fn active_preset(&self) -> crate::domain::preset::ActivePreset {
        self.preset.active()
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
        self.invalidate_layout_cache();
    }

    /// Snap the preset cursor to a specific [`crate::domain::preset::ActivePreset`].
    /// Tests use this to land on a known preset; production code
    /// only ever cycles via [`Self::cycle_preset`], so this method
    /// is `#[cfg(test)]`-gated to keep the public surface small.
    #[cfg(test)]
    pub fn set_active_preset(&mut self, preset: crate::domain::preset::ActivePreset) {
        self.preset.set(preset);
        self.invalidate_layout_cache();
    }

    /// Replace the custom preset's [`Slot`] tree with one parsed
    /// from the DSL string `value`. Always writes to
    /// `self.custom_layout` regardless of the active preset cursor;
    /// the cursor is never touched by this method.
    ///
    /// Returns `Err` if the DSL fails to parse or fails post-parse
    /// validation (duplicate widget kinds, etc.).
    pub fn set_custom_layout(
        &mut self,
        value: &str,
    ) -> Result<(), crate::domain::layout_spec::SlotParseError> {
        self.custom_layout = value.parse()?;
        self.invalidate_layout_cache();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ConfigKey — typed-by-kind schema
// ---------------------------------------------------------------------------
//
// Per-kind sub-enums (`BoolKey`, `IntKey`, `EnumKey`, `StringKey`)
// own the variants for their kind. Each sub-enum exposes only the
// operations that make sense for that kind — `BoolKey::toggle`,
// `IntKey::set`, `EnumKey::set_canonical`, `StringKey::validate` —
// so wrong-kind dispatch is *unrepresentable* in the type system
// (no more "panic if called on the wrong kind" methods).
//
// `ConfigKey` is the user-facing wrapper that the options-menu
// CAT lists carry. Callers pattern-match on it to recover the
// typed inner key:
//
// ```text
// match key {
//     ConfigKey::Bool(k)   => k.toggle(config),
//     ConfigKey::Int(k)    => /* enter int editor */,
//     ConfigKey::Enum(k)   => /* cycle / commit canonical name */,
//     ConfigKey::String(k) => /* enter string editor */,
// }
// ```
//
// `config_schema!` collapses the per-kind sub-enum generation +
// the `ConfigKey` wrapper into one declarative table grouped by
// kind. Each section enumerates its variants with the access shape
// connecting each variant to its `Config` field.
//
// Access shapes (string-only — the other kinds are always
// `field $f`):
//   * `field $f`              — direct `config.$f`
//   * `joined_vec $f`         — `Vec<String>` displayed as a
//                               whitespace-joined string
//   * `array $f [$i]`         — fixed-array element at index `$i`
//   * `custom_layout`         — the layout DSL, virtual key whose
//                               display goes through
//                               `config.layout_spec()` and write
//                               through `config.set_custom_layout()`
//
// Methods that carry per-variant logic (parse with bounds, validate
// with per-key rules, set with per-shape destination) are
// hand-written on the sub-enums after the macro expansion.

macro_rules! config_schema {
    (
        bools {
            $( $bvar:ident => $bname:literal => $bsection:ident . $bfield:ident ),* $(,)?
        }
        ints {
            $( $ivar:ident => $iname:literal => $isection:ident . $ifield:ident ),* $(,)?
        }
        enums {
            $( $evar:ident => $ename:literal => $esection:ident . $efield:ident ),* $(,)?
        }
        strings {
            $( $svar:ident => $sname:literal => { $($sshape:tt)+ } ),* $(,)?
        }
    ) => {
        // === BoolKey ====================================================

        /// Boolean-typed config keys.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum BoolKey { $( $bvar ),* }

        impl BoolKey {
            /// TOML field name (snake_case).
            pub fn name(self) -> &'static str {
                match self { $( Self::$bvar => $bname, )* }
            }

            /// Read the current bool value.
            pub fn get(self, config: &Config) -> bool {
                match self { $( Self::$bvar => config.$bsection.$bfield, )* }
            }

            /// Flip the bool in-place.
            pub fn toggle(self, config: &mut Config) {
                match self {
                    $( Self::$bvar => { config.$bsection.$bfield = !config.$bsection.$bfield; } )*
                }
            }

            /// Display string for the options menu (`"true"` / `"false"`).
            pub fn get_display(self, config: &Config) -> String {
                bool_display(self.get(config))
            }
        }

        // === IntKey =====================================================

        /// Integer-typed config keys.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum IntKey { $( $ivar ),* }

        impl IntKey {
            /// Every variant in declaration order. Generated by the
            /// `config_schema!` macro so adding a new int key
            /// automatically appears here.
            pub const ALL: &'static [IntKey] = &[ $( Self::$ivar ),* ];

            /// TOML field name (snake_case).
            pub fn name(self) -> &'static str {
                match self { $( Self::$ivar => $iname, )* }
            }

            /// Read the current int value.
            pub fn get(self, config: &Config) -> i64 {
                match self { $( Self::$ivar => config.$isection.$ifield, )* }
            }

            /// Write the int value. No clamping — caller should
            /// invoke [`Config::validate`] to apply bounds.
            pub fn set(self, config: &mut Config, value: i64) {
                match self { $( Self::$ivar => { config.$isection.$ifield = value; } )* }
            }

            /// Display string for the options menu (the integer
            /// rendered in base 10).
            pub fn get_display(self, config: &Config) -> String {
                self.get(config).to_string()
            }
        }

        // === EnumKey ====================================================

        /// Typed-enum config keys (closed set of canonical names
        /// enforced by the underlying enum's `FromStr`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum EnumKey { $( $evar ),* }

        impl EnumKey {
            /// TOML field name (snake_case).
            pub fn name(self) -> &'static str {
                match self { $( Self::$evar => $ename, )* }
            }

            /// Display string for the options menu (the enum's
            /// canonical lowercase name via `Display`).
            pub fn get_display(self, config: &Config) -> String {
                match self { $( Self::$evar => config.$esection.$efield.to_string(), )* }
            }
        }

        // === StringKey ==================================================

        /// String-typed config keys (free-form, constrained, list,
        /// or DSL).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StringKey { $( $svar ),* }

        impl StringKey {
            /// TOML field name (snake_case).
            pub fn name(self) -> &'static str {
                match self { $( Self::$svar => $sname, )* }
            }

            /// Display string for the options menu — the field
            /// value rendered for the access shape.
            pub fn get_display(self, config: &Config) -> String {
                match self {
                    $( Self::$svar => config_schema!(@string_display config $($sshape)+), )*
                }
            }
        }

        // === ConfigKey wrapper ==========================================

        /// One config field, identified by its kind plus the typed
        /// sub-enum variant for that kind.
        ///
        /// The options-menu CAT lists carry `&[ConfigKey]`; callers
        /// pattern-match on the wrapper to recover the typed inner.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ConfigKey {
            Bool(BoolKey),
            Int(IntKey),
            Enum(EnumKey),
            String(StringKey),
        }

        impl ConfigKey {
            /// TOML field name (snake_case). Delegates to the
            /// typed inner.
            pub fn name(self) -> &'static str {
                match self {
                    Self::Bool(k) => k.name(),
                    Self::Int(k) => k.name(),
                    Self::Enum(k) => k.name(),
                    Self::String(k) => k.name(),
                }
            }

            /// Returns the kind of value this key holds.
            pub fn kind(self) -> KeyKind {
                match self {
                    Self::Bool(_) => KeyKind::Bool,
                    Self::Int(_) => KeyKind::Int,
                    Self::Enum(_) => KeyKind::Enum,
                    Self::String(_) => KeyKind::String,
                }
            }

            /// Returns a display string for the value of this key
            /// in the given config. Delegates to the typed inner.
            pub fn get_display(self, config: &Config) -> String {
                match self {
                    Self::Bool(k) => k.get_display(config),
                    Self::Int(k) => k.get_display(config),
                    Self::Enum(k) => k.get_display(config),
                    Self::String(k) => k.get_display(config),
                }
            }

            /// Parse a TOML field name into a `ConfigKey`.
            #[cfg(test)]
            pub fn parse(name: &str) -> Option<Self> {
                $( if name == $bname { return Some(Self::Bool(BoolKey::$bvar)); } )*
                $( if name == $iname { return Some(Self::Int(IntKey::$ivar)); } )*
                $( if name == $ename { return Some(Self::Enum(EnumKey::$evar)); } )*
                $( if name == $sname { return Some(Self::String(StringKey::$svar)); } )*
                None
            }
        }

        // From impls let CAT-list authors lift a typed key to the
        // wrapper at the call site without hand-writing the variant
        // wrapper, e.g. `ConfigKey::from(BoolKey::ThemeBackground)`.
        // Const lists cannot use `.into()` (it's not const), so the
        // CAT lists still spell out `ConfigKey::Bool(BoolKey::X)`,
        // but runtime-built collections can use `From`.
        impl From<BoolKey>   for ConfigKey { fn from(k: BoolKey)   -> Self { Self::Bool(k) } }
        impl From<IntKey>    for ConfigKey { fn from(k: IntKey)    -> Self { Self::Int(k) } }
        impl From<EnumKey>   for ConfigKey { fn from(k: EnumKey)   -> Self { Self::Enum(k) } }
        impl From<StringKey> for ConfigKey { fn from(k: StringKey) -> Self { Self::String(k) } }
    };

    // === @string_display: dispatch by access shape ===
    (@string_display $cfg:ident field $section:ident . $f:ident) => { $cfg.$section.$f.clone() };
    (@string_display $cfg:ident joined_vec $section:ident . $f:ident) => { $cfg.$section.$f.join(" ") };
    (@string_display $cfg:ident array $section:ident . $f:ident [$i:literal]) => { $cfg.$section.$f[$i].clone() };
    (@string_display $cfg:ident custom_layout) => { $cfg.custom_layout.to_string() };
}

config_schema! {
    bools {
        ThemeBackground => "theme_background" => ui.theme_background,
        RoundedCorners => "rounded_corners" => ui.rounded_corners,
        ProcReversed => "proc_reversed" => view.proc_reversed,
        ProcTree => "proc_tree" => view.proc_tree,
        ProcColors => "proc_colors" => proc.proc_colors,
        ProcGradient => "proc_gradient" => proc.proc_gradient,
        ProcPerCore => "proc_per_core" => view.proc_per_core,
        ProcMemBytes => "proc_mem_bytes" => proc.proc_mem_bytes,
        ProcAggregate => "proc_aggregate" => proc.proc_aggregate,
        KeepDeadProcUsage => "keep_dead_proc_usage" => proc.keep_dead_proc_usage,
        CpuInvertLower => "cpu_invert_lower" => cpu.cpu_invert_lower,
        CpuSingleGraph => "cpu_single_graph" => cpu.cpu_single_graph,
        CpuAutoScale => "cpu_auto_scale" => cpu.cpu_auto_scale,
        ShowUptime => "show_uptime" => cpu.show_uptime,
        ShowCpuWatts => "show_cpu_watts" => cpu.show_cpu_watts,
        CheckTemp => "check_temp" => cpu.check_temp,
        ShowCoretemp => "show_coretemp" => cpu.show_coretemp,
        ShowCpuFreq => "show_cpu_freq" => cpu.show_cpu_freq,
        ShowSwap => "show_swap" => mem.show_swap,
        ShowIoStat => "show_io_stat" => disk.show_io_stat,
        IoMode => "io_mode" => view.io_mode,
        IoGraphCombined => "io_graph_combined" => disk.io_graph_combined,
        SwapUploadDownload => "swap_upload_download" => net.swap_upload_download,
        Base10Sizes => "base_10_sizes" => ui.base_10_sizes,
        NetAuto => "net_auto" => view.net_auto,
        NetSync => "net_sync" => view.net_sync,
        VimKeys => "vim_keys" => ui.vim_keys,
        BackgroundUpdate => "background_update" => ui.background_update,
        TerminalSync => "terminal_sync" => ui.terminal_sync,
        DiskIoMode => "disk_io_mode" => disk.disk_io_mode,
    }
    ints {
        UpdateMs => "update_ms" => refresh.update_ms,
        CpuUpdateMs => "cpu_update_ms" => refresh.cpu_update_ms,
        MemUpdateMs => "mem_update_ms" => refresh.mem_update_ms,
        DiskUpdateMs => "disk_update_ms" => refresh.disk_update_ms,
        NetUpdateMs => "net_update_ms" => refresh.net_update_ms,
        GpuUpdateMs => "gpu_update_ms" => refresh.gpu_update_ms,
        ProcUpdateMs => "proc_update_ms" => refresh.proc_update_ms,
        NetDownload => "net_download" => net.net_download,
        NetUpload => "net_upload" => net.net_upload,
    }
    enums {
        GraphSymbol => "graph_symbol" => ui.graph_symbol,
        GraphSymbolCpu => "graph_symbol_cpu" => cpu.graph_symbol_cpu,
        GraphSymbolNet => "graph_symbol_net" => net.graph_symbol_net,
        GraphSymbolDisk => "graph_symbol_disk" => disk.graph_symbol_disk,
        ProcSorting => "proc_sorting" => view.proc_sorting,
        CpuGraphUpper => "cpu_graph_upper" => cpu.cpu_graph_upper,
        CpuGraphLower => "cpu_graph_lower" => cpu.cpu_graph_lower,
        TempScale => "temp_scale" => cpu.temp_scale,
        // `log_level` lives in `LogConfig` purely so the macro can
        // address it with the same `section.field` shape it uses
        // for every other key.
        LogLevel => "log_level" => log.log_level,
    }
    strings {
        ColorTheme => "color_theme" => { field ui.color_theme },
        ClockFormat => "clock_format" => { field ui.clock_format },
        CustomCpuName => "custom_cpu_name" => { field cpu.custom_cpu_name },
        ProcFilter => "proc_filter" => { field view.proc_filter },
        DisksFilter => "disks_filter" => { joined_vec disk.disks_filter },
        CustomLayout => "custom_layout" => { custom_layout },
        CustomGpuName0 => "custom_gpu_name0" => { array gpu.custom_gpu_names[0] },
        CustomGpuName1 => "custom_gpu_name1" => { array gpu.custom_gpu_names[1] },
        CustomGpuName2 => "custom_gpu_name2" => { array gpu.custom_gpu_names[2] },
        CustomGpuName3 => "custom_gpu_name3" => { array gpu.custom_gpu_names[3] },
        CustomGpuName4 => "custom_gpu_name4" => { array gpu.custom_gpu_names[4] },
        CustomGpuName5 => "custom_gpu_name5" => { array gpu.custom_gpu_names[5] },
        CustomGpuName6 => "custom_gpu_name6" => { array gpu.custom_gpu_names[6] },
        CustomGpuName7 => "custom_gpu_name7" => { array gpu.custom_gpu_names[7] },
    }
}

// ---------------------------------------------------------------------------
// IntKey — hand-written impls (per-key bounds + step + parse)
// ---------------------------------------------------------------------------

impl IntKey {
    /// Per-key step size used by the options-menu arrow-step path.
    /// `*UpdateMs` keys step in 100 ms increments; throughput caps
    /// step in single-unit increments.
    pub fn step(self) -> i64 {
        match self {
            Self::UpdateMs
            | Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => 100,
            Self::NetDownload | Self::NetUpload => 1,
        }
    }

    /// Inclusive `[min, max]` bounds for this key. The single source
    /// of truth — both [`Self::parse`] (rejecting out-of-range input
    /// from the inline editor) and [`Self::clamp_to_bounds`]
    /// (clamping out-of-range values from a loaded TOML file)
    /// consult this method.
    pub fn bounds(self) -> std::ops::RangeInclusive<i64> {
        match self {
            Self::UpdateMs => 100..=86_400_000,
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => 0..=86_400_000,
            Self::NetDownload | Self::NetUpload => 0..=10_000_000,
        }
    }

    /// Parse `value` as an integer for this key, enforcing the bounds
    /// returned by [`Self::bounds`].
    pub fn parse(self, value: &str) -> Result<i64, &'static str> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("must be an integer");
        }
        let n: i64 = trimmed.parse().map_err(|_| "must be an integer")?;
        if self.bounds().contains(&n) {
            Ok(n)
        } else {
            Err(self.bounds_message())
        }
    }

    /// Clamp `value` into [`Self::bounds`], returning the in-range
    /// value. Used by [`Config::validate`] after loading a TOML file
    /// so out-of-range values land at the nearest valid edge instead
    /// of failing the whole load.
    pub fn clamp_to_bounds(self, value: i64) -> i64 {
        let bounds = self.bounds();
        value.clamp(*bounds.start(), *bounds.end())
    }

    /// Static error message naming the legal range for this key.
    pub fn bounds_message(self) -> &'static str {
        match self {
            Self::UpdateMs => "must be 100..86400000 ms",
            Self::CpuUpdateMs
            | Self::MemUpdateMs
            | Self::DiskUpdateMs
            | Self::NetUpdateMs
            | Self::GpuUpdateMs
            | Self::ProcUpdateMs => "must be 0..86400000 ms (0=inherit)",
            Self::NetDownload | Self::NetUpload => "must be 0..10000000 KiB/s",
        }
    }
}

// ---------------------------------------------------------------------------
// EnumKey — hand-written impls (choices + parse-and-set)
// ---------------------------------------------------------------------------

impl EnumKey {
    /// The closed set of canonical (lowercase) names the menu
    /// cycles for this enum key.
    pub fn choices(self) -> &'static [&'static str] {
        match self {
            Self::GraphSymbol
            | Self::GraphSymbolCpu
            | Self::GraphSymbolNet
            | Self::GraphSymbolDisk => GraphSymbol::NAMES,
            Self::CpuGraphUpper | Self::CpuGraphLower => CpuGraphSource::NAMES,
            Self::TempScale => TempScale::NAMES,
            Self::ProcSorting => ProcSort::NAMES,
            Self::LogLevel => crate::log::FILTER_NAMES,
        }
    }

    /// Parse a canonical name into the typed enum and store it in
    /// `config`. Returns `Err(SetStringError)` if `value` is not in
    /// [`Self::choices`].
    pub fn set_canonical(self, config: &mut Config, value: &str) -> Result<(), SetStringError> {
        let err = || SetStringError {
            key: self.name(),
            value: value.to_string(),
        };
        match self {
            Self::GraphSymbol => config.ui.graph_symbol = value.parse().map_err(|_| err())?,
            Self::GraphSymbolCpu => {
                config.cpu.graph_symbol_cpu = value.parse().map_err(|_| err())?
            }
            Self::GraphSymbolNet => {
                config.net.graph_symbol_net = value.parse().map_err(|_| err())?
            }
            Self::GraphSymbolDisk => {
                config.disk.graph_symbol_disk = value.parse().map_err(|_| err())?
            }
            Self::ProcSorting => config.view.proc_sorting = value.parse().map_err(|_| err())?,
            Self::CpuGraphUpper => config.cpu.cpu_graph_upper = value.parse().map_err(|_| err())?,
            Self::CpuGraphLower => config.cpu.cpu_graph_lower = value.parse().map_err(|_| err())?,
            Self::TempScale => config.cpu.temp_scale = value.parse().map_err(|_| err())?,
            Self::LogLevel => config.log.log_level = value.parse().map_err(|_| err())?,
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// StringKey — hand-written impls (choices, validate, set)
// ---------------------------------------------------------------------------

impl StringKey {
    /// Optional closed set of canonical names. `Some` only for
    /// constrained string keys (today: just `color_theme`); `None`
    /// for free-form keys and the special `disks_filter` /
    /// `custom_layout` parsers.
    pub fn choices(self) -> Option<&'static [&'static str]> {
        match self {
            Self::ColorTheme => Some(crate::theme::THEME_NAMES),
            _ => None,
        }
    }

    /// Validate that `value` is acceptable without mutating the
    /// config. Returns a static error message suitable for inline
    /// display in the options menu when validation fails.
    ///
    /// Used by the inline editor commit path: validate first, only
    /// call [`Self::set`] when validation succeeds.
    pub fn validate(self, value: &str) -> Result<(), &'static str> {
        match self {
            Self::CustomLayout => value
                .parse::<Slot>()
                .map(|_| ())
                .map_err(|_| "invalid layout"),
            Self::DisksFilter => parse_disks_filter(value).map(|_| ()),
            Self::ColorTheme => {
                if self
                    .choices()
                    .expect("ColorTheme has choices by construction")
                    .contains(&value)
                {
                    Ok(())
                } else {
                    Err("invalid value")
                }
            }
            // Free-form string keys accept anything.
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
        }
    }

    /// Set the field from a string. Returns `Err(SetStringError)`
    /// if `value` does not parse for the field's shape (currently
    /// only `CustomLayout` and `DisksFilter` can fail here; other
    /// string keys are free-form or pre-validated by the caller).
    pub fn set(self, config: &mut Config, value: &str) -> Result<(), SetStringError> {
        let err = || SetStringError {
            key: self.name(),
            value: value.to_string(),
        };
        match self {
            Self::ColorTheme => config.ui.color_theme = value.to_string(),
            Self::ClockFormat => config.ui.clock_format = value.to_string(),
            Self::CustomCpuName => config.cpu.custom_cpu_name = value.to_string(),
            Self::ProcFilter => config.view.proc_filter = value.to_string(),
            Self::CustomLayout => {
                config.set_custom_layout(value).map_err(|_| err())?;
            }
            Self::DisksFilter => {
                config.disk.disks_filter = parse_disks_filter(value).map_err(|_| err())?;
            }
            Self::CustomGpuName0 => config.gpu.custom_gpu_names[0] = value.to_string(),
            Self::CustomGpuName1 => config.gpu.custom_gpu_names[1] = value.to_string(),
            Self::CustomGpuName2 => config.gpu.custom_gpu_names[2] = value.to_string(),
            Self::CustomGpuName3 => config.gpu.custom_gpu_names[3] = value.to_string(),
            Self::CustomGpuName4 => config.gpu.custom_gpu_names[4] = value.to_string(),
            Self::CustomGpuName5 => config.gpu.custom_gpu_names[5] = value.to_string(),
            Self::CustomGpuName6 => config.gpu.custom_gpu_names[6] = value.to_string(),
            Self::CustomGpuName7 => config.gpu.custom_gpu_names[7] = value.to_string(),
        }
        Ok(())
    }
}

/// Parse `value` as a whitespace-separated list of drive filter
/// entries (e.g. `"C: !D:"`).
///
/// An empty list is allowed (matches every disk). Each entry must
/// be a single ASCII letter followed by `:`, optionally prefixed
/// with `!`. Returns the original token list (so case and `!`
/// prefixes are preserved verbatim for round-trip fidelity);
/// normalisation happens at match time.
fn parse_disks_filter(value: &str) -> Result<Vec<String>, &'static str> {
    let tokens: Vec<String> = value.split_whitespace().map(str::to_string).collect();
    let parsed = crate::domain::disk::DisksFilter::parse(&tokens);
    if !parsed.invalid().is_empty() {
        return Err("drive entries must be like 'C:' or '!D:'");
    }
    Ok(tokens)
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
    use crate::domain::widget_kind::WidgetKind;

    #[test]
    fn load_empty_file_uses_defaults() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_empty.toml");
        fs::write(&tmp, "").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.refresh.update_ms, 2000);
        assert_eq!(config.ui.color_theme, "default");
        assert!(config.ui.theme_background);
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
        assert_eq!(config.ui.color_theme, "dracula");
        assert!(!config.ui.theme_background);
        assert_eq!(config.refresh.update_ms, 500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn load_preserves_comments() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_comments.toml");
        fs::write(&tmp, "# this is a comment\nupdate_ms = 1000\n").unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty());
        assert_eq!(config.refresh.update_ms, 1000);
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
        assert_eq!(config.refresh.update_ms, 100);
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
        assert_eq!(config.ui.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.ui.color_theme, "default");
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
        assert_eq!(config.ui.color_theme, "default");
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn write_roundtrip_preserves_values() {
        let mut config = Config::new();
        config.ui.color_theme = "nord".to_string();
        config.ui.vim_keys = true;
        config.refresh.update_ms = 1500;

        let tmp = std::env::temp_dir().join("rtop_test_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut config2 = Config::new();
        config2.load(&tmp);
        assert_eq!(config2.ui.color_theme, "nord");
        assert!(config2.ui.vim_keys);
        assert_eq!(config2.refresh.update_ms, 1500);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn defaults_are_correct() {
        let config = Config::new();
        assert!(config.ui.theme_background);
        assert!(!config.ui.vim_keys);
        assert_eq!(config.refresh.update_ms, 2000);
        assert_eq!(config.ui.color_theme, "default");
        assert_eq!(config.ui.graph_symbol, GraphSymbol::Braille);
        assert_eq!(config.view.proc_sorting, ProcSort::Cpu);
        // First-launch cursor lands on the `all` builtin so the
        // user's first `p` press visibly cycles to a different
        // preset, and the cursor name matches the visible layout.
        assert_eq!(
            config.active_preset(),
            crate::domain::preset::ActivePreset::Builtin(crate::domain::preset::BuiltinPreset::All)
        );
        assert_eq!(
            *config.layout_spec(),
            crate::domain::preset::BuiltinPreset::All.layout_spec(),
        );
    }

    /// Round-trip an existing-format `rtop.toml` through load to
    /// verify the per-subsystem split preserves the on-disk
    /// flat-key format. Asserts each section's fields populate
    /// correctly from top-level TOML keys.
    #[test]
    fn load_existing_flat_format_populates_all_sections() {
        let mut config = Config::new();
        let tmp = std::env::temp_dir().join("rtop_test_flat_format.toml");
        fs::write(
            &tmp,
            r#"
color_theme = "dracula"
theme_background = false
vim_keys = true
update_ms = 500
cpu_update_ms = 250
cpu_invert_lower = false
show_uptime = false
custom_cpu_name = "My CPU"
show_swap = false
io_mode = true
disks_filter = ["C:", "!D:"]
net_iface = "Ethernet"
net_download = 5000
proc_tree = true
proc_sorting = "memory"
proc_filter = "chrome"
custom_gpu_names = ["RTX 4090", "", "", "", "", "", "", ""]
log_level = "info"
hidden_widgets = ["mem", "gpu0"]
"#,
        )
        .unwrap();
        let warnings = config.load(&tmp);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        // UiConfig
        assert_eq!(config.ui.color_theme, "dracula");
        assert!(!config.ui.theme_background);
        assert!(config.ui.vim_keys);

        // RefreshConfig
        assert_eq!(config.refresh.update_ms, 500);
        assert_eq!(config.refresh.cpu_update_ms, 250);

        // CpuConfig
        assert!(!config.cpu.cpu_invert_lower);
        assert!(!config.cpu.show_uptime);
        assert_eq!(config.cpu.custom_cpu_name, "My CPU");

        // MemConfig
        assert!(!config.mem.show_swap);

        // DiskConfig
        assert!(config.view.io_mode);
        assert_eq!(config.disk.disks_filter, vec!["C:", "!D:"]);

        // NetConfig
        assert_eq!(config.view.net_iface, "Ethernet");
        assert_eq!(config.net.net_download, 5000);

        // ProcConfig
        assert!(config.view.proc_tree);
        assert_eq!(config.view.proc_sorting, ProcSort::Memory);
        assert_eq!(config.view.proc_filter, "chrome");

        // GpuConfig
        assert_eq!(config.gpu.custom_gpu_names[0], "RTX 4090");
        assert_eq!(config.gpu.custom_gpu_names[1], "");

        // LogConfig
        assert_eq!(config.log.log_level, LevelFilter::INFO);

        // Top-level
        assert!(config.hidden_widgets.contains(WidgetKind::Mem));
        assert!(config.hidden_widgets.contains(WidgetKind::Gpu(0)));

        let _ = fs::remove_file(&tmp);
    }

    /// Round-trip a Config through write+load to verify the
    /// serialised form is itself a valid input — the per-subsystem
    /// split must not introduce any field name drift.
    #[test]
    fn write_and_reload_round_trips_cleanly() {
        let mut original = Config::new();
        original.ui.color_theme = "nord".to_string();
        original.ui.vim_keys = true;
        original.refresh.update_ms = 750;
        original.view.proc_tree = true;
        original.cpu.show_cpu_freq = false;
        original.gpu.custom_gpu_names[0] = "Test GPU 0".to_string();
        original.log.log_level = LevelFilter::DEBUG;

        let tmp = std::env::temp_dir().join("rtop_test_subsystem_roundtrip.toml");
        original.write(&tmp).unwrap();

        let mut loaded = Config::new();
        let warnings = loaded.load(&tmp);
        assert!(warnings.is_empty(), "round-trip warnings: {warnings:?}");

        assert_eq!(loaded.ui.color_theme, "nord");
        assert!(loaded.ui.vim_keys);
        assert_eq!(loaded.refresh.update_ms, 750);
        assert!(loaded.view.proc_tree);
        assert!(!loaded.cpu.show_cpu_freq);
        assert_eq!(loaded.gpu.custom_gpu_names[0], "Test GPU 0");
        assert_eq!(loaded.log.log_level, LevelFilter::DEBUG);

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn validate_clamps_ints() {
        let mut config = Config::new();
        config.refresh.update_ms = 50;
        config.net.net_download = -1;
        let warnings = config.validate();
        assert_eq!(config.refresh.update_ms, 100);
        assert_eq!(config.net.net_download, 0);
        assert!(warnings.len() >= 2);
    }

    #[test]
    fn validate_keeps_valid_disks_filter_unchanged() {
        let mut config = Config::new();
        config.disk.disks_filter = vec!["C:".into(), "!D:".into()];
        let warnings = config.validate();
        assert!(
            !warnings.iter().any(|w| w.contains("disks_filter")),
            "valid disks_filter should not warn, got: {warnings:?}"
        );
        assert_eq!(
            config.disk.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn validate_warns_and_strips_invalid_disks_filter_entries() {
        let mut config = Config::new();
        config.disk.disks_filter = vec![
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
            config.disk.disks_filter,
            vec!["C:".to_string(), "D:".to_string()]
        );
    }

    #[test]
    fn validate_empty_disks_filter_does_not_warn() {
        let mut config = Config::new();
        config.disk.disks_filter = Vec::new();
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("disks_filter")));
        assert!(config.disk.disks_filter.is_empty());
    }

    #[test]
    fn preset_default_is_all_builtin() {
        let config = Config::new();
        assert_eq!(
            config.active_preset(),
            crate::domain::preset::ActivePreset::Builtin(crate::domain::preset::BuiltinPreset::All)
        );
    }

    #[test]
    fn cycle_preset_forward_walks_full_cycle_then_wraps() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config.set_active_preset(ActivePreset::Builtin(BuiltinPreset::All));
        for _ in 0..ActivePreset::CYCLE_LEN {
            config.cycle_preset(true);
        }
        // After one full lap forward we are back where we started.
        assert_eq!(
            config.active_preset(),
            ActivePreset::Builtin(BuiltinPreset::All)
        );
        config.cycle_preset(false);
        // One step backward from "all" lands on Custom (the slot
        // immediately before the first builtin in cycle order).
        assert_eq!(config.active_preset(), ActivePreset::Custom);
    }

    #[test]
    fn cycle_preset_to_builtin_dispatches_to_builtin_layout() {
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        config.custom_layout = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        ]);
        config.set_active_preset(ActivePreset::Custom);
        // On custom, live = custom.
        assert_eq!(*config.layout_spec(), config.custom_layout);

        // Cycle to builtin "all". Live now reads from the builtin.
        config.set_active_preset(ActivePreset::Builtin(BuiltinPreset::All));
        assert_eq!(*config.layout_spec(), BuiltinPreset::All.layout_spec());
        assert!(config.layout_spec().contains(WidgetKind::Cpu));
        assert!(config.layout_spec().contains(WidgetKind::Disk));
        // Custom storage is untouched by cycling.
        assert_eq!(
            config.custom_layout,
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
        config.custom_layout = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
            Slot::Widget(WidgetKind::Gpu(0)),
        ]);
        config.set_active_preset(ActivePreset::Custom);

        // Visit a builtin and come back.
        config.cycle_preset(true);
        config.cycle_preset(false);
        assert_eq!(config.active_preset(), ActivePreset::Custom);
        assert_eq!(
            *config.layout_spec(),
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
            config.active_preset(),
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
        use crate::domain::preset::ActivePreset;
        let mut config = Config::new();
        // Pin the cursor to Custom so the live layout reads from
        // custom.root after the round-trip — first-launch default
        // is the `all` builtin, which would otherwise mask
        // custom.root entirely.
        config.set_active_preset(ActivePreset::Custom);
        config.custom_layout = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        let tmp = std::env::temp_dir().join("rtop_test_layout_roundtrip.toml");
        config.write(&tmp).unwrap();

        let mut loaded = Config::new();
        loaded.load(&tmp);
        assert_eq!(loaded.active_preset(), ActivePreset::Custom);
        assert_eq!(
            *loaded.layout_spec(),
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
            ConfigKey::String(StringKey::ColorTheme),
            ConfigKey::Enum(EnumKey::ProcSorting),
            ConfigKey::Bool(BoolKey::ThemeBackground),
            ConfigKey::Int(IntKey::UpdateMs),
            ConfigKey::Int(IntKey::NetDownload),
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
        assert_eq!(parse_disks_filter("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_disks_filter("   ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn parse_disks_filter_preserves_case_and_prefix() {
        let result = parse_disks_filter("c: !D: E:").unwrap();
        assert_eq!(
            result,
            vec!["c:".to_string(), "!D:".to_string(), "E:".to_string()]
        );
    }

    #[test]
    fn parse_disks_filter_rejects_bare_letter() {
        assert!(parse_disks_filter("C").is_err());
        assert!(parse_disks_filter("C: D").is_err());
        assert!(parse_disks_filter("!E").is_err());
    }

    #[test]
    fn parse_int_enforces_update_ms_lower_bound() {
        assert!(IntKey::UpdateMs.parse("99").is_err());
        assert_eq!(IntKey::UpdateMs.parse("100").unwrap(), 100);
    }

    #[test]
    fn parse_int_accepts_zero_for_inherit_keys() {
        for key in [
            IntKey::CpuUpdateMs,
            IntKey::MemUpdateMs,
            IntKey::DiskUpdateMs,
            IntKey::NetUpdateMs,
            IntKey::GpuUpdateMs,
            IntKey::ProcUpdateMs,
        ] {
            assert_eq!(key.parse("0").unwrap(), 0);
        }
    }

    #[test]
    fn parse_int_rejects_negative_and_overflow() {
        assert!(IntKey::CpuUpdateMs.parse("-1").is_err());
        assert!(IntKey::NetDownload.parse("10000001").is_err());
        assert_eq!(IntKey::NetDownload.parse("10000000").unwrap(), 10_000_000);
    }

    #[test]
    fn parse_int_trims_whitespace() {
        assert_eq!(IntKey::UpdateMs.parse("  500  ").unwrap(), 500);
    }

    #[test]
    fn parse_int_rejects_non_numeric_or_empty() {
        assert!(IntKey::UpdateMs.parse("abc").is_err());
        assert!(IntKey::UpdateMs.parse("").is_err());
    }

    // Note: the previous `parse_int_rejects_non_int_key` test is no
    // longer expressible — `parse` lives on `IntKey`, so passing a
    // non-int key is a compile error rather than a runtime check.

    #[test]
    fn validate_string_shape() {
        assert!(StringKey::CustomLayout.validate("vstack(cpu, mem)").is_ok());
        assert!(StringKey::CustomLayout.validate("cpu").is_ok());
        assert!(StringKey::CustomLayout.validate("").is_err());
        assert!(StringKey::CustomLayout.validate("nope").is_err());
        assert!(
            StringKey::CustomLayout
                .validate("vstack(cpu, cpu)")
                .is_err()
        );
    }

    #[test]
    fn validate_string_disks_filter() {
        assert!(StringKey::DisksFilter.validate("").is_ok());
        assert!(StringKey::DisksFilter.validate("C: !D:").is_ok());
        assert!(StringKey::DisksFilter.validate("X").is_err());
    }

    #[test]
    fn validate_string_free_form_keys_always_ok() {
        for key in [
            StringKey::ClockFormat,
            StringKey::CustomCpuName,
            StringKey::ProcFilter,
            StringKey::CustomGpuName0,
            StringKey::CustomGpuName7,
        ] {
            assert!(key.validate("").is_ok());
            assert!(key.validate("anything goes !@#$%").is_ok());
        }
    }

    #[test]
    fn validate_string_constrained_choice_keys() {
        // ColorTheme is the one constrained string key — `validate`
        // checks membership in the bundled theme list.
        assert!(StringKey::ColorTheme.validate("default").is_ok());
        assert!(StringKey::ColorTheme.validate("nonexistent").is_err());
    }

    #[test]
    fn enum_set_canonical_accepts_choices_rejects_unknown() {
        // Enum keys validate via `set_canonical` (they don't have a
        // standalone `validate` method — they're always parsed via
        // `FromStr` of the typed enum).
        let mut config = Config::new();
        assert!(EnumKey::LogLevel.set_canonical(&mut config, "info").is_ok());
        assert!(
            EnumKey::LogLevel
                .set_canonical(&mut config, "loud")
                .is_err()
        );
    }

    // Note: the previous `validate_string_rejects_non_string_keys`
    // test is no longer expressible — `validate` lives on
    // `StringKey`, so passing a Bool or Int key is a compile error.

    #[test]
    fn set_custom_layout_writes_to_custom_root_without_touching_cursor_from_builtin() {
        // From a builtin cursor, set_custom_layout mutates `custom.root`
        // without moving the cursor or affecting the active layout.
        // The user must explicitly cycle to Custom to see the
        // result of their edit.
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        let cursor = ActivePreset::Builtin(BuiltinPreset::All);
        config.set_active_preset(cursor);
        let active_before = config.layout_spec().clone();
        config.set_custom_layout("vstack(cpu, mem)").unwrap();
        // Cursor unchanged.
        assert_eq!(config.active_preset(), cursor);
        // Active layout unchanged (still the builtin).
        assert_eq!(*config.layout_spec(), active_before);
        // But custom.root captured the edit.
        assert_eq!(
            config.custom_layout,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
    }

    #[test]
    fn set_custom_layout_on_custom_writes_root_and_changes_active_layout() {
        use crate::domain::preset::ActivePreset;
        let mut config = Config::new();
        config.set_active_preset(ActivePreset::Custom);
        config.set_custom_layout("cpu").unwrap();
        assert_eq!(config.active_preset(), ActivePreset::Custom);
        assert_eq!(config.custom_layout, Slot::Widget(WidgetKind::Cpu));
        assert_eq!(*config.layout_spec(), Slot::Widget(WidgetKind::Cpu));
    }

    #[test]
    fn set_string_custom_layout_via_inline_editor_path() {
        // Inline editor commit goes through `StringKey::CustomLayout::set`
        // which calls `Config::set_custom_layout`. Same no-promote
        // semantics.
        use crate::domain::preset::{ActivePreset, BuiltinPreset};
        let mut config = Config::new();
        let cursor = ActivePreset::Builtin(BuiltinPreset::CpuProc);
        config.set_active_preset(cursor);
        StringKey::CustomLayout
            .set(&mut config, "vstack(cpu, mem, disk)")
            .unwrap();
        // Cursor unchanged.
        assert_eq!(config.active_preset(), cursor);
        // custom.root captured the edit.
        assert_eq!(
            config.custom_layout,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Disk),
            ])
        );
    }

    #[test]
    fn set_string_custom_layout_invalid_returns_err() {
        let mut config = Config::new();
        assert!(StringKey::CustomLayout.set(&mut config, "nope").is_err());
        assert!(StringKey::CustomLayout.set(&mut config, "").is_err());
        // Duplicate widget kinds rejected at parse time.
        assert!(
            StringKey::CustomLayout
                .set(&mut config, "vstack(cpu, cpu)")
                .is_err()
        );
    }

    #[test]
    fn set_string_disks_filter_via_inline_editor_path() {
        let mut config = Config::new();
        StringKey::DisksFilter.set(&mut config, "C: !D:").unwrap();
        assert_eq!(
            config.disk.disks_filter,
            vec!["C:".to_string(), "!D:".to_string()]
        );
    }

    #[test]
    fn set_string_disks_filter_invalid_returns_err() {
        let mut config = Config::new();
        assert!(StringKey::DisksFilter.set(&mut config, "X").is_err());
    }
}
