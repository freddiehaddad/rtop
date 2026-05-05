//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit the `custom_*` fields in `rtop.toml` directly or
//! cycle to the custom preset (index `BUILTIN_PRESETS.len()`).
//! The only preset state persisted across runs is
//! `Config::current_preset` (the index into `BUILTIN_PRESETS` plus
//! the trailing custom slot).

use crate::domain::widget_kind::WidgetKind;

/// Layout configuration for a single named preset.
///
/// Each preset is a complete description of which widgets are shown
/// and how the orientation-sensitive widgets are positioned. Cycling
/// to a preset overwrites the live layout view returned by
/// `Config::widgets()` / `cpu_bottom()` / `mem_below_net()` /
/// `proc_left()`. Other Config fields are untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Short human-readable label.
    pub name: &'static str,
    /// Widget kinds in display order.
    pub widgets: &'static [WidgetKind],
    /// `true` to render the CPU widget at the bottom of the screen
    /// instead of the top.
    pub cpu_bottom: bool,
    /// `true` to position the memory widget below the network widget
    /// instead of above.
    pub mem_below_net: bool,
    /// `true` to render the process widget on the left side instead
    /// of the right.
    pub proc_left: bool,
}

/// All presets that ship with rtop, in cycle order.
///
/// Index 0 is the launch default when `Config::current_preset` is
/// unset, out of range, or invalid. Index `BUILTIN_PRESETS.len()`
/// represents the user's mutable "custom" preset, whose layout is
/// stored in `Config::custom_*` fields rather than here.
pub const BUILTIN_PRESETS: &[Preset] = &[
    Preset {
        name: "all",
        widgets: &[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "cpu+proc",
        widgets: &[WidgetKind::Cpu, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "cpu+mem+disk",
        widgets: &[WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Disk],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "cpu+net+proc",
        widgets: &[WidgetKind::Cpu, WidgetKind::Net, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_is_non_empty() {
        assert!(!BUILTIN_PRESETS.is_empty());
    }

    #[test]
    fn builtin_presets_have_unique_names() {
        let mut names: Vec<&str> = BUILTIN_PRESETS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "preset names must be unique");
    }

    #[test]
    fn builtin_presets_have_at_least_one_widget() {
        for preset in BUILTIN_PRESETS {
            assert!(
                !preset.widgets.is_empty(),
                "preset '{}' must reference at least one widget",
                preset.name,
            );
        }
    }

    #[test]
    fn builtin_preset_zero_is_all_widgets() {
        let p = &BUILTIN_PRESETS[0];
        assert_eq!(p.name, "all");
        assert_eq!(
            p.widgets,
            &[
                WidgetKind::Cpu,
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk,
            ]
        );
        assert!(!p.cpu_bottom);
        assert!(!p.mem_below_net);
        assert!(!p.proc_left);
    }
}
