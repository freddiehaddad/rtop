//! Persisted configuration state and the typed schema for editing it.
//!
//! The persisted shape is composed of per-subsystem sections (see
//! [`sections`] — `UiConfig`, `RefreshConfig`, ..., `ViewConfig`,
//! `LogConfig`) flattened into a single TOML root. The active preset
//! cursor and the custom-layout DSL are encapsulated in
//! [`CachedLayoutSpec`] so the resolved-layout cache cannot drift
//! from its inputs.
//!
//! The typed schema for editing config fields lives in [`schema`]
//! ([`BoolKey`], [`IntKey`], [`EnumKey`], [`StringKey`], and the
//! [`ConfigKey`] wrapper). Adding or modifying a key is a one-table
//! edit in the `config_schema!` invocation in `schema.rs`.

mod schema;
mod sections;
#[cfg(test)]
mod tests;

pub use schema::*;
pub use sections::*;

use crate::domain::layout_spec::Slot;
use crate::domain::preset::default_custom_layout;
use sections::validate_choice;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

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
// CachedLayoutSpec — preset + custom layout + cache, with invalidation
// invariants encoded in the type
// ---------------------------------------------------------------------------

/// The active preset cursor, the persisted custom layout, and the
/// memoised resolved layout, packaged together so the cache cannot
/// fall out of sync with its inputs.
///
/// Fields are private; the only mutators are [`Self::cycle`],
/// [`Self::set_custom`], and (test-only) [`Self::set_active_preset`].
/// Each invalidates the cache before returning. Read access is via
/// [`Self::spec`] (the resolved layout), [`Self::active_preset`] (the
/// cursor), and [`Self::custom_layout`] (the persisted custom slot).
///
/// On the wire (`#[serde(flatten)]` from [`Config`]) `preset` and
/// `custom_layout` appear at the TOML root, matching the historical
/// flat layout. The `cache` field is `#[serde(skip)]` and starts
/// empty on every load.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CachedLayoutSpec {
    /// Cursor over the active preset (one of the builtins, or the
    /// custom slot). Persisted by canonical name.
    preset: crate::domain::preset::PresetField,
    /// Persisted layout for the `Custom` preset slot, in DSL form.
    custom_layout: Slot,
    /// Cached resolved layout. Materialised by [`Self::spec`] from
    /// `preset` + `custom_layout`. Invalidated by every mutator.
    #[serde(skip)]
    cache: std::cell::OnceCell<Slot>,
}

impl Default for CachedLayoutSpec {
    fn default() -> Self {
        Self {
            preset: crate::domain::preset::PresetField::default(),
            custom_layout: default_custom_layout(),
            cache: std::cell::OnceCell::new(),
        }
    }
}

impl CachedLayoutSpec {
    /// Read the active preset cursor.
    pub fn active_preset(&self) -> crate::domain::preset::ActivePreset {
        self.preset.active()
    }

    /// Borrow the persisted custom-preset layout. Always returns the
    /// `Custom` slot regardless of which preset is currently active.
    pub fn custom_layout(&self) -> &Slot {
        &self.custom_layout
    }

    /// Build (or retrieve) the resolved active layout. Cached after
    /// first call; mutators clear the cache.
    pub fn spec(&self) -> &Slot {
        self.cache.get_or_init(|| match self.preset.active() {
            crate::domain::preset::ActivePreset::Builtin(b) => b.layout_spec(),
            crate::domain::preset::ActivePreset::Custom => self.custom_layout.clone(),
        })
    }

    /// Move the preset cursor one position in the cycle. `forward = true`
    /// advances (`p`); `false` retreats (`P`). Always invalidates the
    /// cache.
    pub fn cycle(&mut self, forward: bool) {
        let next = if forward {
            self.preset.active().next()
        } else {
            self.preset.active().prev()
        };
        self.preset.set(next);
        self.invalidate();
    }

    /// Replace the custom-preset slot tree. Always invalidates the
    /// cache. The cursor is not touched.
    pub fn set_custom(&mut self, slot: Slot) {
        self.custom_layout = slot;
        self.invalidate();
    }

    /// Snap the preset cursor to a specific [`crate::domain::preset::ActivePreset`].
    /// Test-only; production code only ever cycles via [`Self::cycle`].
    #[cfg(test)]
    pub fn set_active_preset(&mut self, preset: crate::domain::preset::ActivePreset) {
        self.preset.set(preset);
        self.invalidate();
    }

    /// Surface a deserialise-time invalid preset name (if any). The
    /// caller folds this into the validation warning list.
    pub fn take_invalid_preset(&mut self) -> Option<String> {
        self.preset.take_invalid()
    }

    fn invalidate(&mut self) {
        self.cache = std::cell::OnceCell::new();
    }
}

// ---------------------------------------------------------------------------
// Config — composed wrapper over per-subsystem sections
// ---------------------------------------------------------------------------

/// All persisted configuration state for rtop.
///
/// Composed of per-subsystem sections (see [`sections`]) flattened
/// into a single TOML root via `#[serde(flatten)]`. Top-level fields
/// (`layout`, `hidden_widgets`, `conf_file`) stay on the wrapper
/// because they don't fit a single subsystem.
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
    pub disk: DiskConfig,
    #[serde(flatten)]
    pub log: LogConfig,
    /// View-state (runtime-toggle) section. Mirrored at runtime
    /// by [`crate::app::RuntimeView`]; see [`ViewConfig`] for the
    /// sync-point contract.
    #[serde(flatten)]
    pub view: ViewConfig,

    /// Statusbar widget settings (master visibility, per-item
    /// visibility, clock format). See [`StatusbarConfig`].
    #[serde(flatten)]
    pub statusbar: StatusbarConfig,

    /// Active preset cursor + persisted custom layout + memoised
    /// resolved layout, packaged so the cache cannot drift from its
    /// inputs. `preset` and `custom_layout` flatten to the TOML root;
    /// the resolved-layout cache is `#[serde(skip)]` and starts empty
    /// every load.
    #[serde(flatten)]
    pub layout: CachedLayoutSpec,

    /// Persisted runtime view filter. The user toggles widgets in
    /// or out of this set with `1`-`6` / `Shift+R`; the set
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
            disk: DiskConfig::default(),
            log: LogConfig::default(),
            view: ViewConfig::default(),
            statusbar: StatusbarConfig::default(),

            // First-launch cursor lands on the `all` builtin so the
            // very first `p` keypress visibly cycles to a different
            // preset, and the cursor name matches the visible
            // layout. Custom is reached by cycling all the way
            // around or by editing the layout (toggle keys, options
            // menu, or `custom_layout` DSL string in `rtop.toml`),
            // at which point the cursor auto-promotes.
            //
            // First-launch custom layout: a clone of the `all`
            // preset's tree (see `default_custom_layout`). The
            // cursor is on `all` builtin by default so this slot is
            // dormant until the user edits something.
            layout: CachedLayoutSpec::default(),

            // First-launch view filter is empty — every widget the
            // active preset exposes is visible. The field is
            // persisted so toggle gestures (1-9, 0, Shift+R)
            // survive restart.
            hidden_widgets: crate::domain::widget_set::WidgetSet::new(),

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
        if let Some(invalid) = self.layout.take_invalid_preset() {
            warnings.push(format!(
                "Unknown preset name in 'preset': '{invalid}', falling back to custom",
            ));
        }

        // Validate disk_filter: surface invalid drive entries as
        // warnings and drop them in place so the saved config
        // matches what the runtime actually uses.
        let filter = crate::domain::disk::DiskFilter::parse(&self.disk.disk_filter);
        if !filter.invalid().is_empty() {
            warnings.push(format!(
                "Invalid drive entry/entries in 'disk_filter': {}",
                filter.invalid().join(", ")
            ));
            let invalid_tokens = filter.invalid().to_vec();
            self.disk
                .disk_filter
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
    /// Delegates to [`CachedLayoutSpec::spec`] so the cache cannot
    /// drift from `preset` / `custom_layout`.
    pub fn layout_spec(&self) -> &Slot {
        self.layout.spec()
    }

    /// Read the active preset cursor.
    pub fn active_preset(&self) -> crate::domain::preset::ActivePreset {
        self.layout.active_preset()
    }

    /// Move the preset cursor one position in the cycle. `forward`
    /// = `true` advances (`p`); `forward` = `false` retreats (`P`).
    pub fn cycle_preset(&mut self, forward: bool) {
        self.layout.cycle(forward);
    }

    /// Snap the preset cursor to a specific [`crate::domain::preset::ActivePreset`].
    /// Tests use this to land on a known preset; production code
    /// only ever cycles via [`Self::cycle_preset`].
    #[cfg(test)]
    pub fn set_active_preset(&mut self, preset: crate::domain::preset::ActivePreset) {
        self.layout.set_active_preset(preset);
    }

    /// Replace the custom preset's [`Slot`] tree with one parsed
    /// from the DSL string `value`. Always writes through to
    /// `self.layout`'s custom slot regardless of the active preset
    /// cursor; the cursor is never touched by this method.
    ///
    /// Returns `Err` if the DSL fails to parse or fails post-parse
    /// validation (duplicate widget kinds, etc.).
    pub fn set_custom_layout(
        &mut self,
        value: &str,
    ) -> Result<(), crate::domain::layout_spec::SlotParseError> {
        let slot: Slot = value.parse()?;
        self.layout.set_custom(slot);
        Ok(())
    }
}
