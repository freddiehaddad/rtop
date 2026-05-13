//! Integration tests for Config (load/write/validate, schema dispatch,
//! preset cycling, custom layout DSL round-trip).
//!
//! Lives in its own file purely so mod.rs does not carry ~700 lines of
//! tests on top of the production code; the test module remains gated
//! by `#[cfg(test)]` from mod.rs declaration.

use super::schema::parse_disk_filter;
use super::*;
use crate::collect::process_display::ProcSort;
use crate::domain::config_enums::GraphSymbol;
use crate::domain::widget_kind::WidgetKind;
use tracing_subscriber::filter::LevelFilter;

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
statusbar_show_uptime = false
custom_cpu_name = "My CPU"
show_swap = false
io_mode = true
disk_filter = ["C:", "!D:"]
net_iface = "{12345678-1234-1234-1234-123456789012}"
net_download = 5000
proc_tree = true
proc_sorting = "memory"
proc_filter = "chrome"
gpu_iface = "NVIDIA:GPU-aaaa"
log_level = "info"
hidden_widgets = ["mem", "gpu"]
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
    assert_eq!(config.cpu.custom_cpu_name, "My CPU");

    // StatusbarConfig
    assert!(!config.statusbar.statusbar_show_uptime);

    // MemConfig
    assert!(!config.mem.show_swap);

    // DiskConfig
    assert!(config.view.io_mode);
    assert_eq!(config.disk.disk_filter, vec!["C:", "!D:"]);

    // NetConfig
    assert_eq!(
        config.view.net_iface,
        "{12345678-1234-1234-1234-123456789012}"
    );
    assert_eq!(config.net.net_download, 5000);

    // ProcConfig
    assert!(config.view.proc_tree);
    assert_eq!(config.view.proc_sorting, ProcSort::Memory);
    assert_eq!(config.view.proc_filter, "chrome");

    // GPU view (cycling-GPU widget persists the last-viewed
    // device's stable id; no per-device labels exist).
    assert_eq!(config.view.gpu_iface, "NVIDIA:GPU-aaaa");

    // LogConfig
    assert_eq!(config.log.log_level, LevelFilter::INFO);

    // Top-level
    assert!(config.hidden_widgets.contains(WidgetKind::Mem));
    assert!(config.hidden_widgets.contains(WidgetKind::Gpu));

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
    original.view.gpu_iface = "AMD:UDID-deadbeef".to_string();
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
    assert_eq!(loaded.view.gpu_iface, "AMD:UDID-deadbeef");
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
fn validate_keeps_valid_disk_filter_unchanged() {
    let mut config = Config::new();
    config.disk.disk_filter = vec!["C:".into(), "!D:".into()];
    let warnings = config.validate();
    assert!(
        !warnings.iter().any(|w| w.contains("disk_filter")),
        "valid disk_filter should not warn, got: {warnings:?}"
    );
    assert_eq!(
        config.disk.disk_filter,
        vec!["C:".to_string(), "!D:".to_string()]
    );
}

#[test]
fn validate_warns_and_strips_invalid_disk_filter_entries() {
    let mut config = Config::new();
    config.disk.disk_filter = vec![
        "C:".into(),
        "abc".into(),
        "D:".into(),
        "3".into(),
        "!!".into(),
    ];
    let warnings = config.validate();
    let disks_warning = warnings
        .iter()
        .find(|w| w.contains("disk_filter"))
        .expect("validate must surface a disk_filter warning");
    assert!(disks_warning.contains("abc"));
    assert!(disks_warning.contains('3'));
    assert!(disks_warning.contains("!!"));
    assert_eq!(
        config.disk.disk_filter,
        vec!["C:".to_string(), "D:".to_string()]
    );
}

#[test]
fn validate_empty_disk_filter_does_not_warn() {
    let mut config = Config::new();
    config.disk.disk_filter = Vec::new();
    let warnings = config.validate();
    assert!(!warnings.iter().any(|w| w.contains("disk_filter")));
    assert!(config.disk.disk_filter.is_empty());
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
    config.layout.set_custom(Slot::VStack(vec![
        Slot::Widget(WidgetKind::Mem),
        Slot::Widget(WidgetKind::Proc),
    ]));
    config.set_active_preset(ActivePreset::Custom);
    // On custom, live = custom.
    assert_eq!(config.layout_spec(), config.layout.custom_layout());

    // Cycle to builtin "all". Live now reads from the builtin.
    config.set_active_preset(ActivePreset::Builtin(BuiltinPreset::All));
    assert_eq!(*config.layout_spec(), BuiltinPreset::All.layout_spec());
    assert!(config.layout_spec().contains(WidgetKind::Cpu));
    assert!(config.layout_spec().contains(WidgetKind::Disk));
    // Custom storage is untouched by cycling.
    assert_eq!(
        config.layout.custom_layout(),
        &Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        ])
    );
}

#[test]
fn cycle_preset_back_to_custom_restores_user_layout() {
    use crate::domain::preset::ActivePreset;
    let mut config = Config::new();
    config.layout.set_custom(Slot::VStack(vec![
        Slot::Widget(WidgetKind::Mem),
        Slot::Widget(WidgetKind::Proc),
        Slot::Widget(WidgetKind::Gpu),
    ]));
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
            Slot::Widget(WidgetKind::Gpu),
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
    config.layout.set_custom(Slot::VStack(vec![
        Slot::Widget(WidgetKind::Cpu),
        Slot::Widget(WidgetKind::Mem),
    ]));
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
fn parse_disk_filter_accepts_empty() {
    assert_eq!(parse_disk_filter("").unwrap(), Vec::<String>::new());
    assert_eq!(parse_disk_filter("   ").unwrap(), Vec::<String>::new());
}

#[test]
fn parse_disk_filter_preserves_case_and_prefix() {
    let result = parse_disk_filter("c: !D: E:").unwrap();
    assert_eq!(
        result,
        vec!["c:".to_string(), "!D:".to_string(), "E:".to_string()]
    );
}

#[test]
fn parse_disk_filter_rejects_bare_letter() {
    assert!(parse_disk_filter("C").is_err());
    assert!(parse_disk_filter("C: D").is_err());
    assert!(parse_disk_filter("!E").is_err());
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
fn gpu_update_ms_round_trips_through_field() {
    // The cycling-GPU widget uses a single global interval — the
    // schema exposes one `IntKey::GpuUpdateMs` reading and writing
    // the scalar `refresh.gpu_update_ms` field.
    let mut config = crate::config::Config::new();
    IntKey::GpuUpdateMs.set(&mut config, 1234);
    assert_eq!(IntKey::GpuUpdateMs.get(&config), 1234);
    assert_eq!(config.refresh.gpu_update_ms, 1234);
}

// ---------------------------------------------------------------------------
// RefreshConfig::effective — the single resolver of the
// "0 = inherit global" rule. Behavior is byte-identical to the
// (now-removed) Config::effective_interval. The tests below lock
// every branch and bound: zero / negative widget_ms, zero / sub-100
// global, mid-range overrides, override floor (100), override ceiling
// (86_400_000), and over-ceiling clamping.
// ---------------------------------------------------------------------------

fn refresh_with_global(global: i64) -> RefreshConfig {
    RefreshConfig {
        update_ms: global,
        ..RefreshConfig::default()
    }
}

#[test]
fn refresh_effective_zero_widget_ms_returns_global() {
    let r = refresh_with_global(2000);
    assert_eq!(r.effective(0), 2000);
}

#[test]
fn refresh_effective_negative_widget_ms_returns_global() {
    let r = refresh_with_global(2000);
    assert_eq!(r.effective(-1), 2000);
    assert_eq!(r.effective(i64::MIN), 2000);
}

#[test]
fn refresh_effective_zero_global_floors_at_100() {
    // A 0 (or negative) global would cause the worker loop's
    // recv_timeout(Duration::from_millis(0)) to busy-spin. The
    // resolver pins the floor at 100 ms before that ever happens.
    let r = refresh_with_global(0);
    assert_eq!(r.effective(0), 100);
    let r = refresh_with_global(-500);
    assert_eq!(r.effective(0), 100);
}

#[test]
fn refresh_effective_sub_100_global_floors_at_100() {
    let r = refresh_with_global(50);
    assert_eq!(r.effective(0), 100);
    let r = refresh_with_global(99);
    assert_eq!(r.effective(0), 100);
    let r = refresh_with_global(100);
    assert_eq!(r.effective(0), 100);
}

#[test]
fn refresh_effective_widget_ms_passes_through() {
    let r = refresh_with_global(2000);
    assert_eq!(r.effective(250), 250);
    assert_eq!(r.effective(500), 500);
    assert_eq!(r.effective(1500), 1500);
}

#[test]
fn refresh_effective_widget_ms_floors_at_100() {
    let r = refresh_with_global(2000);
    assert_eq!(r.effective(1), 100);
    assert_eq!(r.effective(99), 100);
    assert_eq!(r.effective(100), 100);
}

#[test]
fn refresh_effective_widget_ms_ceilings_at_86_400_000() {
    let r = refresh_with_global(2000);
    assert_eq!(r.effective(86_400_000), 86_400_000);
    assert_eq!(r.effective(86_400_001), 86_400_000);
    assert_eq!(r.effective(i64::MAX), 86_400_000);
}

#[test]
fn refresh_effective_widget_override_independent_of_global() {
    // The override path does NOT consult `update_ms`. A widget with
    // its own non-zero override polls at that override regardless of
    // any global value (sub-100 global, huge global, anything).
    for global in [0, 50, 100, 2000, 86_400_000, i64::MAX] {
        let r = refresh_with_global(global);
        assert_eq!(r.effective(750), 750, "global={global}");
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
fn validate_string_disk_filter() {
    assert!(StringKey::DiskFilter.validate("").is_ok());
    assert!(StringKey::DiskFilter.validate("C: !D:").is_ok());
    assert!(StringKey::DiskFilter.validate("X").is_err());
}

#[test]
fn validate_string_free_form_keys_always_ok() {
    for key in [
        StringKey::StatusbarClockFormat,
        StringKey::CustomCpuName,
        StringKey::ProcFilter,
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
        config.layout.custom_layout(),
        &Slot::VStack(vec![
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
    assert_eq!(
        config.layout.custom_layout(),
        &Slot::Widget(WidgetKind::Cpu)
    );
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
        config.layout.custom_layout(),
        &Slot::VStack(vec![
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
fn set_string_disk_filter_via_inline_editor_path() {
    let mut config = Config::new();
    StringKey::DiskFilter.set(&mut config, "C: !D:").unwrap();
    assert_eq!(
        config.disk.disk_filter,
        vec!["C:".to_string(), "!D:".to_string()]
    );
}

#[test]
fn set_string_disk_filter_invalid_returns_err() {
    let mut config = Config::new();
    assert!(StringKey::DiskFilter.set(&mut config, "X").is_err());
}
