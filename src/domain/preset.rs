//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit `shown_boxes` and the position bools in `rtop.toml`
//! directly. The only preset state persisted across runs is
//! `Config::current_preset` (the index into `BUILTIN_PRESETS`),
//! which is bumped each time the user cycles.

/// Layout configuration for a single named preset.
///
/// Each preset is a complete description of which boxes are shown
/// and how the orientation-sensitive boxes are positioned. Cycling
/// to a preset overwrites `Config::shown_boxes`, `cpu_bottom`,
/// `mem_below_net`, and `proc_left`. Other Config fields are
/// untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Short human-readable label, shown in the CPU box's preset
    /// indicator inset (e.g. `preset *cpu+proc`).
    pub name: &'static str,
    /// Box names in display order. Each entry must satisfy
    /// `crate::config::is_valid_box_name` (asserted at startup
    /// via the `builtin_presets_only_reference_valid_boxes`
    /// test).
    pub boxes: &'static [&'static str],
    /// `true` to render the CPU box at the bottom of the screen
    /// instead of the top.
    pub cpu_bottom: bool,
    /// `true` to position the memory box below the network box
    /// instead of above.
    pub mem_below_net: bool,
    /// `true` to render the process box on the left side instead
    /// of the right.
    pub proc_left: bool,
}

/// All presets that ship with rtop, in cycle order.
///
/// Index 0 is the launch default when `Config::current_preset` is
/// unset, out of range, or invalid.
pub const BUILTIN_PRESETS: &[Preset] = &[
    Preset {
        name: "cpu+proc",
        boxes: &["cpu", "proc"],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "cpu+mem+disk",
        boxes: &["cpu", "mem", "disk"],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "cpu+net+proc",
        boxes: &["cpu", "net", "proc"],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
    },
    Preset {
        name: "all",
        boxes: &["cpu", "mem", "net", "proc", "disk"],
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
    fn builtin_presets_have_at_least_one_box() {
        for preset in BUILTIN_PRESETS {
            assert!(
                !preset.boxes.is_empty(),
                "preset '{}' must reference at least one box",
                preset.name,
            );
        }
    }

    #[test]
    fn builtin_presets_only_reference_valid_boxes() {
        for preset in BUILTIN_PRESETS {
            for box_name in preset.boxes {
                assert!(
                    crate::config::is_valid_box_name(box_name),
                    "preset '{}' references invalid box name '{}'",
                    preset.name,
                    box_name,
                );
            }
        }
    }
}
